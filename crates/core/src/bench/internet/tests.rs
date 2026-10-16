use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use super::*;

/// A just-enough `LibreSpeed` backend on a local port: it answers pings and the address lookup,
/// serves a megabyte or two for each download request, and takes whatever is posted.
fn backend() -> Server {
    backend_taking("Local", u64::MAX)
}

/// The same, but refusing any upload bigger than `limit` bytes, as a web server with a small
/// body limit does.
fn backend_taking(name: &str, limit: u64) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/backend", listener.local_addr().unwrap());
    thread::spawn(move || {
        for connection in listener.incoming().flatten() {
            thread::spawn(move || drop(serve(connection, limit)));
        }
    });
    Server::from(Listed {
        id: 1,
        name: name.to_owned(),
        server: base,
        download: "garbage.php".to_owned(),
        upload: "empty.php".to_owned(),
        ping: "empty.php".to_owned(),
        address: "getIP.php".to_owned(),
    })
}

fn serve(stream: TcpStream, limit: u64) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let mut request = String::new();
        if reader.read_line(&mut request)? == 0 {
            return Ok(());
        }
        let target = request.split_whitespace().nth(1).unwrap_or("").to_owned();
        let (mut length, mut chunked) = (0_u64, false);
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            let header = header.trim().to_ascii_lowercase();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            chunked |= header.starts_with("transfer-encoding:") && header.contains("chunked");
        }
        let mut received = length;
        if chunked {
            received = 0;
            loop {
                let mut size = String::new();
                reader.read_line(&mut size)?;
                let size = u64::from_str_radix(size.trim(), 16).unwrap_or(0);
                io::copy(&mut reader.by_ref().take(size + 2), &mut io::sink())?;
                received += size;
                if size == 0 {
                    break;
                }
            }
        } else {
            io::copy(&mut reader.by_ref().take(length), &mut io::sink())?;
        }
        if received > limit {
            writer.write_all(
                b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
            )?;
            continue;
        }
        let body = if target.contains("garbage.php") {
            vec![0_u8; 2 << 20]
        } else if target.contains("getIP.php") {
            br#"{"processedString":"203.0.113.9 - Test ISP, ZZ (1 km)","rawIspInfo":null}"#.to_vec()
        } else {
            Vec::new()
        };
        write!(
            writer,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        )?;
        writer.write_all(&body)?;
    }
}

const QUICK: Plan = Plan {
    download_grace: Duration::from_millis(100),
    upload_grace: Duration::from_millis(100),
    measure: Duration::from_millis(300),
    stagger: Duration::from_millis(20),
};

#[test]
fn a_run_against_a_server_measures_everything() {
    let stop = AtomicBool::new(false);
    let result = measure(Some(&backend()), &|_| {}, &stop, &QUICK).unwrap();
    assert!(result.latency_ms > 0.0 && result.loss_percent.abs() < f32::EPSILON);
    assert!(result.download.single > 0 && result.download.multi > 0);
    assert!(result.upload.single > 0 && result.upload.multi > 0);
    assert!(result.loaded_download_ms.is_some() && result.loaded_upload_ms.is_some());
    assert_eq!(result.address.as_deref(), Some("203.0.113.9"));
    assert_eq!(result.isp.as_deref(), Some("Test ISP, ZZ"));
}

#[test]
fn the_address_is_not_saved_with_the_result() {
    let stop = AtomicBool::new(false);
    let result = measure(Some(&backend()), &|_| {}, &stop, &QUICK).unwrap();
    let saved = serde_json::to_string(&result).unwrap();
    assert!(!saved.contains("203.0.113.9"));
    let loaded: Internet = serde_json::from_str(&saved).unwrap();
    assert_eq!(loaded.address, None);
    assert_eq!(loaded.isp, result.isp);
}

#[test]
fn a_stopped_run_posts_no_result() {
    let stop = AtomicBool::new(true);
    assert!(measure(Some(&backend()), &|_| {}, &stop, &QUICK).is_err());
}

#[test]
fn a_server_that_answers_nothing_is_an_error_not_a_zero() {
    let silent = Server::from(Listed {
        id: 2,
        name: "Silent".to_owned(),
        server: "http://127.0.0.1:9".to_owned(),
        download: "d".to_owned(),
        upload: "u".to_owned(),
        ping: "p".to_owned(),
        address: "a".to_owned(),
    });
    let stop = AtomicBool::new(false);
    let error = measure(Some(&silent), &|_| {}, &stop, &QUICK).unwrap_err();
    assert!(error.contains("Silent"), "{error}");
}

#[test]
fn the_nearest_server_is_the_one_that_answers_soonest() {
    let stop = AtomicBool::new(false);
    let working = backend();
    let dead = Server {
        name: "Dead".to_owned(),
        ping: "http://127.0.0.1:9/p".to_owned(),
        ..working.clone()
    };
    assert_eq!(nearest(&[dead, working.clone()], &stop).unwrap(), [working]);
    assert!(nearest(&[], &stop).is_err());
}

#[test]
fn a_server_that_refuses_the_uploads_is_passed_over_for_the_next_nearest() {
    let stop = AtomicBool::new(false);
    let strict = backend_taking("Strict", 1 << 20);
    let roomy = backend();
    let chosen = first_usable(&[strict, roomy.clone()], &stop).unwrap();
    assert_eq!(chosen, roomy);
}

#[test]
fn when_no_server_can_be_used_the_reasons_are_given() {
    let stop = AtomicBool::new(false);
    let error = first_usable(&[backend_taking("Strict", 1 << 20)], &stop).unwrap_err();
    assert!(error.contains("Strict") && error.contains("413"), "{error}");
}

#[test]
fn a_server_picked_by_hand_that_refuses_the_uploads_says_so_before_measuring() {
    let stop = AtomicBool::new(false);
    let strict = backend_taking("Strict", 1 << 20);
    let error = measure(Some(&strict), &|_| {}, &stop, &QUICK).unwrap_err();
    assert!(
        error.contains("Strict") && error.contains("payload too large"),
        "{error}"
    );
}

#[test]
fn a_listed_server_joins_its_addresses_whether_or_not_the_base_ends_in_a_slash() {
    let listed: Vec<Listed> = serde_json::from_str(
        r#"[{"id":106,"name":"Belgrade","server":"https://speedtest1.sox.rs/librespeed/",
              "dlURL":"backend/garbage.php","ulURL":"backend/empty.php",
              "pingURL":"backend/empty.php","getIpURL":"backend/getIP.php",
              "sponsorName":"SOX","sponsorURL":"https://sox.rs"},
             {"id":51,"name":"Amsterdam","server":"https://ams.speedtest.clouvider.net/backend",
              "dlURL":"garbage.php","ulURL":"empty.php","pingURL":"empty.php","getIpURL":"getIP.php"}]"#,
    )
    .unwrap();
    let servers: Vec<Server> = listed.into_iter().map(Server::from).collect();
    assert_eq!(
        servers[0].download,
        "https://speedtest1.sox.rs/librespeed/backend/garbage.php"
    );
    assert_eq!(
        servers[1].address,
        "https://ams.speedtest.clouvider.net/backend/getIP.php"
    );
}

#[test]
fn the_identity_line_splits_into_address_and_isp() {
    assert_eq!(
        parse_identity("203.0.113.9 - Example Net, US (12 km)"),
        (
            Some("203.0.113.9".to_owned()),
            Some("Example Net, US".to_owned())
        )
    );
    assert_eq!(
        parse_identity("203.0.113.9 - Unknown ISP"),
        (Some("203.0.113.9".to_owned()), None)
    );
    assert_eq!(parse_identity("private IPv4 access"), (None, None));
}

#[test]
fn the_median_is_the_middle_value() {
    assert!((median(&mut [30.0, 10.0, 20.0]) - 20.0).abs() < f32::EPSILON);
    assert!(median(&mut []).abs() < f32::EPSILON);
}

#[test]
fn jitter_is_the_mean_step_between_neighbouring_round_trips() {
    assert!((jitter(&[10.0, 12.0, 9.0]) - 2.5).abs() < f32::EPSILON);
    assert!(jitter(&[10.0]).abs() < f32::EPSILON);
    assert!(jitter(&[250.0; 6]).abs() < f32::EPSILON);
}

#[test]
fn loss_is_the_share_of_pings_that_never_came_back() {
    assert!((loss(&[Some(1.0), None, Some(2.0), None]) - 50.0).abs() < f32::EPSILON);
    assert!(loss(&[]).abs() < f32::EPSILON);
}

#[test]
#[ignore = "uses the network"]
fn the_public_list_has_servers_and_one_of_them_answers() {
    let stop = AtomicBool::new(false);
    let servers = servers().unwrap();
    assert!(servers.len() > 3);
    assert!(nearest(&servers, &stop).is_ok());
}
