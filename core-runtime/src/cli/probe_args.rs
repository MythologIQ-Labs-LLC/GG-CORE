//! Strict CLI argument handling for local read-only health probes (#159).
//!
//! The public help has always advertised --socket and health --json. Do
//! not ignore these flags and accidentally probe the wrong daemon.
use super::get_socket_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeArgs {
    pub socket: String,
    pub json: bool,
}

pub fn parse_probe_args(args: &[String], allow_json: bool) -> Result<ProbeArgs, &'static str> {
    let (mut socket, mut json) = (None, false);
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--socket" if socket.is_none() => {
                index += 1;
                let path = args.get(index).ok_or("--socket requires a path")?;
                if path.is_empty() || path.starts_with("--") {
                    return Err("invalid --socket path");
                }
                socket = Some(path.clone());
            }
            "--json" if allow_json && !json => {
                json = true;
            }
            _ => return Err("unrecognized or repeated probe option"),
        }
        index += 1;
    }
    Ok(ProbeArgs {
        socket: socket.unwrap_or_else(get_socket_path),
        json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(parts: &[&str], allow_json: bool) -> Result<ProbeArgs, &'static str> {
        let args: Vec<String> = parts.iter().map(|arg| (*arg).to_string()).collect();
        parse_probe_args(&args, allow_json)
    }

    #[test]
    fn default_probe_path_is_not_empty() {
        let args = parse(&[], false).unwrap();
        assert!(!args.socket.is_empty());
        assert!(!args.json);
    }

    #[test]
    fn explicit_socket_and_health_json_are_preserved_in_any_order() {
        let a = parse(&["--json", "--socket", "/private/gg.sock"], true).unwrap();
        let b = parse(&["--socket", "/private/gg.sock", "--json"], true).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.socket, "/private/gg.sock");
        assert!(a.json);
        assert!(
            !parse(&["--socket", "/private/gg.sock"], false)
                .unwrap()
                .json
        );
    }

    #[test]
    fn missing_duplicate_and_unsupported_flags_fail_loud() {
        for args in [
            vec!["--socket"],
            vec!["--socket", ""],
            vec!["--socket", "--json"],
            vec!["--socket", "/a", "--socket", "/b"],
            vec!["--json", "--json"],
            vec!["--unknown"],
        ] {
            assert!(parse(&args, true).is_err(), "{args:?}");
        }
        assert!(parse(&["--json"], false).is_err());
    }
}
