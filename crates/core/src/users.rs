use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

const PASSWD: &str = "/etc/passwd";
/// Where systemd's user database services listen, each on a socket of its own.
const USERDB: &str = "/run/systemd/userdb";
const METHOD: &str = "io.systemd.UserDatabase.GetUserRecord";
/// Long enough for a local service, short enough that a stuck one cannot hold the sampler.
const PATIENCE: Duration = Duration::from_millis(200);
/// More than any user record, so a service that never stops talking cannot fill memory.
const LIMIT: u64 = 64 * 1024;

/// The names of the users that own processes, by numeric ID. A name comes from `/etc/passwd` or,
/// for the users systemd makes for services, from the user database services it runs. Only the
/// name and the ID are kept of what either one says.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Users {
    names: HashMap<u32, String>,
    /// IDs nothing could name, so they are not asked about again.
    unknown: HashSet<u32>,
}

impl Users {
    #[must_use]
    pub fn name(&self, uid: u32) -> Option<&str> {
        self.names.get(&uid).map(String::as_str)
    }

    /// Finds the names of those of `uids` not known yet.
    pub fn learn(&mut self, uids: impl IntoIterator<Item = u32>) {
        self.learn_from(Path::new(PASSWD), Path::new(USERDB), uids);
    }

    fn learn_from(&mut self, passwd: &Path, userdb: &Path, uids: impl IntoIterator<Item = u32>) {
        let wanted: HashSet<u32> = uids
            .into_iter()
            .filter(|uid| !self.names.contains_key(uid) && !self.unknown.contains(uid))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let file = fs::read_to_string(passwd).unwrap_or_default();
        self.names
            .extend(parse(&file).filter(|(uid, _)| wanted.contains(uid)));
        let sockets = sockets(userdb);
        for uid in wanted {
            if self.names.contains_key(&uid) {
                continue;
            }
            match sockets.iter().find_map(|socket| lookup(socket, uid)) {
                Some(name) => {
                    self.names.insert(uid, name);
                }
                None => {
                    self.unknown.insert(uid);
                }
            }
        }
    }
}

/// `name:password:uid:gid:...`; comment and malformed lines are skipped.
fn parse(passwd: &str) -> impl Iterator<Item = (u32, String)> {
    passwd.lines().filter_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let uid = fields.nth(1)?.parse().ok()?;
        Some((uid, name.to_owned()))
    })
}

/// The user database services that are running, which are the sockets in the directory.
fn sockets(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_socket()))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

#[derive(Deserialize)]
struct Reply {
    parameters: Option<Answer>,
}

#[derive(Deserialize)]
struct Answer {
    record: Record,
}

/// The one thing wanted of a user record, which also holds a home directory, a shell and more.
#[derive(Deserialize)]
struct Record {
    #[serde(rename = "userName")]
    name: String,
    uid: u32,
}

/// Asks one user database service for the user with `uid`, in the Varlink protocol: a JSON
/// request and a JSON reply, each ended by a zero byte. `None` when it has no such user, does not
/// answer in time, or answers with anything else.
fn lookup(socket: &Path, uid: u32) -> Option<String> {
    let service = socket.file_name()?.to_str()?;
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(PATIENCE)).ok()?;
    stream.set_write_timeout(Some(PATIENCE)).ok()?;
    let request = serde_json::json!({
        "method": METHOD,
        "parameters": { "uid": uid, "service": service },
    });
    stream.write_all(format!("{request}\0").as_bytes()).ok()?;
    let mut reply = Vec::new();
    BufReader::new(stream.take(LIMIT))
        .read_until(0, &mut reply)
        .ok()?;
    reply.pop_if(|byte| *byte == 0)?;
    let record = serde_json::from_slice::<Reply>(&reply)
        .ok()?
        .parameters?
        .record;
    (record.uid == uid).then_some(record.name)
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::thread::{self, JoinHandle};

    use super::*;

    const PASSWD_FILE: &str = "root:x:0:0:root:/root:/bin/bash\n# comment\nweegs:x:1000:100::/home/weegs:/bin/nu\nbroken\n";

    /// A directory of its own for one test, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir().join(format!("peepers-{test}-{}", std::process::id()));
            drop(fs::remove_dir_all(&path));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    /// A user database service that answers one request with `reply` and hands back what it was asked.
    fn serve(directory: &Path, name: &str, reply: &'static str) -> JoinHandle<String> {
        let listener = UnixListener::bind(directory.join(name)).unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            BufReader::new(&stream).read_until(0, &mut request).unwrap();
            stream.write_all(reply.as_bytes()).unwrap();
            stream.write_all(&[0]).unwrap();
            String::from_utf8(request).unwrap()
        })
    }

    const FOUND: &str = r#"{"parameters":{"record":{"userName":"dnscrypt-proxy","uid":62582,"gid":62582,"homeDirectory":"/","shell":"/sbin/nologin","service":"io.systemd.DynamicUser"}}}"#;

    #[test]
    fn names_are_keyed_by_uid() {
        let names: HashMap<_, _> = parse(PASSWD_FILE).collect();
        assert_eq!(names.get(&0).map(String::as_str), Some("root"));
        assert_eq!(names.get(&1000).map(String::as_str), Some("weegs"));
        assert_eq!(names.get(&7), None);
    }

    #[test]
    fn only_the_asked_for_users_are_kept_from_the_file() {
        let scratch = Scratch::new("kept");
        let passwd = scratch.0.join("passwd");
        fs::write(&passwd, PASSWD_FILE).unwrap();
        let mut users = Users::default();
        users.learn_from(&passwd, &scratch.0.join("none"), [1000]);
        assert_eq!(users.name(1000), Some("weegs"));
        assert_eq!(users.name(0), None);
    }

    #[test]
    fn a_service_user_is_named_by_the_database_that_made_it() {
        let scratch = Scratch::new("service");
        let asked = serve(&scratch.0, "io.systemd.DynamicUser", FOUND);
        let mut users = Users::default();
        users.learn_from(&scratch.0.join("passwd"), &scratch.0, [62582]);
        assert_eq!(users.name(62582), Some("dnscrypt-proxy"));
        let request: serde_json::Value =
            serde_json::from_str(asked.join().unwrap().trim_end_matches('\0')).unwrap();
        assert_eq!(request["method"], METHOD);
        assert_eq!(request["parameters"]["uid"], 62582);
        assert_eq!(request["parameters"]["service"], "io.systemd.DynamicUser");
    }

    #[test]
    fn a_user_nothing_knows_is_not_asked_for_again() {
        let scratch = Scratch::new("unknown");
        let asked = serve(
            &scratch.0,
            "io.systemd.DynamicUser",
            r#"{"error":"io.systemd.UserDatabase.NoRecordFound"}"#,
        );
        let mut users = Users::default();
        users.learn_from(&scratch.0.join("passwd"), &scratch.0, [7]);
        asked.join().unwrap();
        assert_eq!(users.name(7), None);
        assert!(users.unknown.contains(&7));
        users.learn_from(&scratch.0.join("passwd"), &scratch.0, [7]);
        assert_eq!(users.name(7), None);
    }

    #[test]
    fn a_record_for_another_user_is_not_taken() {
        let scratch = Scratch::new("other");
        let asked = serve(&scratch.0, "io.systemd.DynamicUser", FOUND);
        assert_eq!(lookup(&scratch.0.join("io.systemd.DynamicUser"), 5), None);
        asked.join().unwrap();
    }

    #[test]
    fn a_service_that_does_not_answer_gives_up() {
        let scratch = Scratch::new("silent");
        let listener = UnixListener::bind(scratch.0.join("io.systemd.Silent")).unwrap();
        let silent = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            thread::sleep(PATIENCE * 3);
            drop(stream);
        });
        assert_eq!(lookup(&scratch.0.join("io.systemd.Silent"), 5), None);
        silent.join().unwrap();
    }

    #[test]
    fn only_sockets_are_user_database_services() {
        let scratch = Scratch::new("sockets");
        fs::write(scratch.0.join("a-file"), "").unwrap();
        let _listener = UnixListener::bind(scratch.0.join("io.systemd.Machine")).unwrap();
        assert_eq!(sockets(&scratch.0), [scratch.0.join("io.systemd.Machine")]);
        assert!(sockets(&scratch.0.join("missing")).is_empty());
    }

    #[test]
    fn root_exists_on_this_machine() {
        let mut users = Users::default();
        users.learn([0]);
        assert_eq!(users.name(0), Some("root"));
    }
}
