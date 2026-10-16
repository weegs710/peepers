use std::collections::HashMap;
use std::fs;

/// User names by numeric ID, from `/etc/passwd`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Users(HashMap<u32, String>);

impl Users {
    #[must_use]
    pub fn read() -> Self {
        Self::parse(&fs::read_to_string("/etc/passwd").unwrap_or_default())
    }

    #[must_use]
    pub fn name(&self, uid: u32) -> Option<&str> {
        self.0.get(&uid).map(String::as_str)
    }

    /// `name:password:uid:gid:...`; comment and malformed lines are skipped.
    fn parse(passwd: &str) -> Self {
        Self(
            passwd
                .lines()
                .filter_map(|line| {
                    let mut fields = line.split(':');
                    let name = fields.next()?;
                    let uid = fields.nth(1)?.parse().ok()?;
                    Some((uid, name.to_owned()))
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n# comment\nweegs:x:1000:100::/home/weegs:/bin/nu\nbroken\n";

    #[test]
    fn names_are_keyed_by_uid() {
        let users = Users::parse(PASSWD);
        assert_eq!(users.name(0), Some("root"));
        assert_eq!(users.name(1000), Some("weegs"));
        assert_eq!(users.name(7), None);
    }

    #[test]
    fn root_exists_on_this_machine() {
        assert_eq!(Users::read().name(0), Some("root"));
    }
}
