//! Identifies who owns a listening socket: Docker, Homebrew, Postgres.app,
//! one of our own instances, or something else.

use std::path::{Path, PathBuf};

use crate::ports::{BindAddr, Listener};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerPort {
    pub container: String,
    pub host: BindAddr,
    pub host_port: u16,
}

/// Parses `docker ps --format '{{.Names}}\t{{.Ports}}'`.
pub fn parse_docker_ps(output: &str) -> Vec<DockerPort> {
    let mut ports = Vec::new();
    for line in output.lines() {
        let Some((name, mappings)) = line.split_once('\t') else {
            continue;
        };
        for mapping in mappings.split(", ") {
            // e.g. "0.0.0.0:5433->5432/tcp" or "[::]:5433->5432/tcp"
            let Some((host, _)) = mapping.split_once("->") else {
                continue;
            };
            let Some((addr, port)) = host.rsplit_once(':') else {
                continue;
            };
            let ipv6 = addr.starts_with('[');
            let (Some(host), Ok(host_port)) = (BindAddr::from_host(addr, ipv6), port.parse())
            else {
                continue;
            };
            ports.push(DockerPort {
                container: name.to_string(),
                host,
                host_port,
            });
        }
    }
    ports
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Docker { containers: Vec<String> },
    Homebrew { formula: Option<String> },
    PostgresApp,
    PgDesk { instance: String },
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct OwnerContext {
    pub brew_prefix: Option<PathBuf>,
    pub docker_ports: Vec<DockerPort>,
    /// (instance name, data dir) of instances managed by PG Desk.
    pub managed: Vec<(String, PathBuf)>,
}

const DOCKER_COMMANDS: [&str; 4] = ["com.docker", "vpnkit", "docker-proxy", "OrbStack"];

pub fn classify(listener: &Listener, command_line: &str, ctx: &OwnerContext) -> Owner {
    if let Some((name, _)) = ctx
        .managed
        .iter()
        .find(|(_, dir)| mentions_dir(command_line, dir))
    {
        return Owner::PgDesk {
            instance: name.clone(),
        };
    }
    if DOCKER_COMMANDS
        .iter()
        .any(|c| listener.command.starts_with(c))
    {
        let mut containers: Vec<String> = ctx
            .docker_ports
            .iter()
            .filter(|p| p.host_port == listener.port)
            .map(|p| p.container.clone())
            .collect();
        containers.dedup();
        return Owner::Docker { containers };
    }
    if let Some(prefix) = ctx.brew_prefix.as_deref().and_then(Path::to_str)
        && let Some(rest) = command_line.strip_prefix(prefix)
    {
        let formula = rest
            .split('/')
            .skip_while(|seg| *seg != "Cellar" && *seg != "opt")
            .nth(1)
            .map(str::to_string);
        return Owner::Homebrew { formula };
    }
    if command_line.contains("/Postgres.app/") {
        return Owner::PostgresApp;
    }
    Owner::Other
}

/// True when `dir` appears in `command_line` as a whole path (quoted or not).
fn mentions_dir(command_line: &str, dir: &Path) -> bool {
    let Some(dir) = dir.to_str() else {
        return false;
    };
    command_line.match_indices(dir).any(|(i, _)| {
        matches!(
            command_line[i + dir.len()..].chars().next(),
            None | Some(' ' | '"' | '\'' | '/')
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{BindAddr, Listener};

    const PS: &str = "kyzn-crm-postgres\t127.0.0.1:5434->5432/tcp\nrekamedika-postgres\t0.0.0.0:5433->5432/tcp, [::]:5433->5432/tcp\n";

    fn listener(command: &str, port: u16) -> Listener {
        Listener {
            pid: 7,
            command: command.into(),
            addr: BindAddr::AnyV6,
            port,
        }
    }

    fn ctx() -> OwnerContext {
        OwnerContext {
            brew_prefix: Some("/opt/homebrew".into()),
            docker_ports: parse_docker_ps(PS),
            managed: vec![(
                "app".into(),
                "/Users/u/Library/Application Support/pgdesk/clusters/app".into(),
            )],
        }
    }

    #[test]
    fn parses_docker_ps_multi_mapping() {
        let ports = parse_docker_ps(PS);
        assert_eq!(ports.len(), 3);
        assert_eq!(
            ports[0],
            DockerPort {
                container: "kyzn-crm-postgres".into(),
                host: BindAddr::LoopbackV4,
                host_port: 5434
            }
        );
        assert_eq!(ports[1].host, BindAddr::AnyV4);
        assert_eq!(ports[2].host, BindAddr::AnyV6);
        assert_eq!(ports[2].host_port, 5433);
    }

    #[test]
    fn docker_ps_ignores_unpublished_ports() {
        assert!(parse_docker_ps("redis\t6379/tcp\nempty\t\n").is_empty());
    }

    #[test]
    fn docker_backend_named_by_container() {
        let owner = classify(
            &listener("com.docker.backend", 5433),
            "/Applications/Docker.app/x",
            &ctx(),
        );
        assert_eq!(
            owner,
            Owner::Docker {
                containers: vec!["rekamedika-postgres".into()]
            }
        );
    }

    #[test]
    fn homebrew_formula_from_cellar_path() {
        let cmd = "/opt/homebrew/Cellar/postgresql@16/16.14/bin/postgres -D /opt/homebrew/var/postgresql@16";
        assert_eq!(
            classify(&listener("postgres", 5432), cmd, &ctx()),
            Owner::Homebrew {
                formula: Some("postgresql@16".into())
            }
        );
    }

    #[test]
    fn homebrew_formula_from_opt_path() {
        let cmd = "/opt/homebrew/opt/postgresql@17/bin/postgres -D /x";
        assert_eq!(
            classify(&listener("postgres", 5432), cmd, &ctx()),
            Owner::Homebrew {
                formula: Some("postgresql@17".into())
            }
        );
    }

    #[test]
    fn pgdesk_owner_matches_data_dir_with_spaces() {
        let cmd =
            "/x/bin/postgres -D /Users/u/Library/Application Support/pgdesk/clusters/app -p 5440";
        assert_eq!(
            classify(&listener("postgres", 5440), cmd, &ctx()),
            Owner::PgDesk {
                instance: "app".into()
            }
        );
    }

    #[test]
    fn pgdesk_owner_does_not_match_prefix_of_other_dir() {
        let cmd = "/x/bin/postgres -D /Users/u/Library/Application Support/pgdesk/clusters/app2";
        assert_eq!(
            classify(&listener("postgres", 5440), cmd, &ctx()),
            Owner::Other
        );
    }

    #[test]
    fn postgres_app_detected() {
        let cmd = "/Applications/Postgres.app/Contents/Versions/17/bin/postgres -D /x";
        assert_eq!(
            classify(&listener("postgres", 5432), cmd, &ctx()),
            Owner::PostgresApp
        );
    }

    #[test]
    fn unknown_is_other() {
        assert_eq!(
            classify(&listener("nc", 5432), "nc -l 5432", &ctx()),
            Owner::Other
        );
    }

    #[test]
    fn no_brew_prefix_never_homebrew() {
        let mut c = ctx();
        c.brew_prefix = None;
        let cmd = "/opt/homebrew/Cellar/postgresql@16/16.14/bin/postgres";
        assert_eq!(classify(&listener("postgres", 5432), cmd, &c), Owner::Other);
    }
}
