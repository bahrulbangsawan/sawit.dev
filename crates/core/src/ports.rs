//! Listening-socket model, `lsof` parsing and port conflict analysis.

use std::net::IpAddr;

/// Address a socket is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindAddr {
    AnyV4,
    /// `*` on an IPv6 socket. Treated as dual-stack because `lsof` does not
    /// show `IPV6_V6ONLY`; when unsure we prefer to warn.
    AnyV6,
    LoopbackV4,
    LoopbackV6,
    Specific(IpAddr),
}

impl BindAddr {
    /// Which address families (v4, v6) this bind can receive traffic for.
    pub fn families(&self) -> (bool, bool) {
        match self {
            BindAddr::AnyV4 | BindAddr::LoopbackV4 => (true, false),
            BindAddr::AnyV6 => (true, true),
            BindAddr::LoopbackV6 => (false, true),
            BindAddr::Specific(ip) => (ip.is_ipv4(), ip.is_ipv6()),
        }
    }

    pub fn is_wildcard(&self) -> bool {
        matches!(self, BindAddr::AnyV4 | BindAddr::AnyV6)
    }

    fn overlaps(&self, other: &BindAddr) -> bool {
        let (a4, a6) = self.families();
        let (b4, b6) = other.families();
        (a4 && b4) || (a6 && b6)
    }

    pub(crate) fn from_host(host: &str, ipv6: bool) -> Option<BindAddr> {
        if host == "*" {
            return Some(if ipv6 {
                BindAddr::AnyV6
            } else {
                BindAddr::AnyV4
            });
        }
        let ip: IpAddr = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .ok()?;
        Some(match ip {
            ip if ip.is_unspecified() && ip.is_ipv4() => BindAddr::AnyV4,
            ip if ip.is_unspecified() => BindAddr::AnyV6,
            ip if ip.is_loopback() && ip.is_ipv4() => BindAddr::LoopbackV4,
            ip if ip.is_loopback() => BindAddr::LoopbackV6,
            ip => BindAddr::Specific(ip),
        })
    }
}

impl std::fmt::Display for BindAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindAddr::AnyV4 => f.write_str("0.0.0.0"),
            BindAddr::AnyV6 => f.write_str("[::]"),
            BindAddr::LoopbackV4 => f.write_str("127.0.0.1"),
            BindAddr::LoopbackV6 => f.write_str("[::1]"),
            BindAddr::Specific(IpAddr::V6(ip)) => write!(f, "[{ip}]"),
            BindAddr::Specific(ip) => write!(f, "{ip}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    pub command: String,
    pub addr: BindAddr,
    pub port: u16,
}

/// Parses `lsof -nP -iTCP:<port> -sTCP:LISTEN -F pctn` output.
pub fn parse_lsof(output: &str) -> Vec<Listener> {
    let mut listeners = Vec::new();
    let (mut pid, mut command, mut ipv6) = (0u32, String::new(), false);
    for line in output.lines() {
        let Some(field) = line.chars().next() else {
            continue;
        };
        let value = &line[field.len_utf8()..];
        match field {
            'p' => {
                pid = value.parse().unwrap_or(0);
                command.clear();
            }
            'c' => command = value.to_string(),
            'f' => ipv6 = false,
            't' => ipv6 = value == "IPv6",
            'n' => {
                let Some((host, port)) = value.rsplit_once(':') else {
                    continue;
                };
                let (Some(addr), Ok(port)) = (BindAddr::from_host(host, ipv6), port.parse()) else {
                    continue;
                };
                listeners.push(Listener {
                    pid,
                    command: command.clone(),
                    addr,
                    port,
                });
            }
            _ => {}
        }
    }
    listeners
}

/// One process from `ps -axo pid=,ppid=,command=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// The command line (argv joined by spaces).
    pub args: String,
}

/// Parses `ps -axo pid=,ppid=,command=` output.
pub fn parse_ps(output: &str) -> Vec<Proc> {
    output
        .lines()
        .filter_map(|line| {
            // Columns are padded with runs of spaces.
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, args) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(Proc {
                pid: pid.parse().ok()?,
                ppid: ppid.parse().ok()?,
                args: args.trim().to_string(),
            })
        })
        .collect()
}

/// Package managers, runtimes, task runners and dev servers: what sits
/// between a terminal and the process that opens a port. Climbing the
/// process tree through these finds the command the user typed.
const LAUNCHERS: &[&str] = &[
    // JavaScript
    "npm",
    "npx",
    "pnpm",
    "pnpx",
    "yarn",
    "yarnpkg",
    "bun",
    "bunx",
    "deno",
    "node",
    "nodejs",
    "tsx",
    "ts-node",
    "nodemon",
    "turbo",
    "nx",
    "lerna",
    "concurrently",
    "npm-run-all",
    "run-p",
    "run-s",
    "vite",
    "next",
    "nuxt",
    "nuxi",
    "astro",
    "remix",
    "react-scripts",
    "webpack",
    "webpack-dev-server",
    "esbuild",
    "parcel",
    "rspack",
    "rsbuild",
    "ng",
    "vue-cli-service",
    "svelte-kit",
    "wrangler",
    "vercel",
    "netlify",
    "firebase",
    "expo",
    "storybook",
    // Python
    "python",
    "uv",
    "uvx",
    "poetry",
    "pipenv",
    "pdm",
    "hatch",
    "flask",
    "uvicorn",
    "gunicorn",
    "hypercorn",
    "daphne",
    "django-admin",
    "fastapi",
    "streamlit",
    "jupyter",
    // Ruby, PHP, JVM, .NET, Go, Rust, Elixir, others
    "ruby",
    "bundle",
    "rails",
    "rake",
    "puma",
    "foreman",
    "overmind",
    "honcho",
    "php",
    "composer",
    "artisan",
    "symfony",
    "java",
    "gradle",
    "gradlew",
    "mvn",
    "mvnw",
    "sbt",
    "dotnet",
    "go",
    "air",
    "cargo",
    "cargo-watch",
    "watchexec",
    "mix",
    "elixir",
    "iex",
    "make",
    "just",
    "task",
    "hugo",
    "jekyll",
    "zola",
    "docker-compose",
    "tilt",
];

/// Interpreters that run a launcher given as a script (`node …/bin/pnpm`).
const INTERPRETERS: &[&str] = &["node", "nodejs", "bun", "deno", "python", "ruby", "php"];

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "fish", "ksh", "nu"];

/// Servers developers run locally that aren't launched through a tool.
const DEV_DAEMONS: &[&str] = &[
    "postgres",
    "postmaster",
    "mysqld",
    "mariadbd",
    "redis-server",
    "valkey-server",
    "mongod",
    "memcached",
    "nginx",
    "httpd",
    "caddy",
    "traefik",
    "minio",
    "clickhouse",
    "elasticsearch",
    "ollama",
    "temporal",
    "mailpit",
    "mailhog",
    "localstack",
];

/// The command's name: basename of argv0, without a version suffix
/// (`python3.12` → `python`) or a login-shell dash (`-zsh` → `zsh`).
fn program(args: &str) -> &str {
    let argv0 = args.split_whitespace().next().unwrap_or_default();
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.trim_start_matches('-');
    base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
        .trim_end_matches('-')
        .pipe_nonempty(base)
}

trait NonEmptyOr {
    fn pipe_nonempty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl NonEmptyOr for str {
    fn pipe_nonempty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

/// The tool a command line runs: its program, or for an interpreter
/// running a launcher script (`node /opt/homebrew/bin/pnpm dev`), that
/// launcher.
pub fn tool(args: &str) -> &str {
    let name = program(args);
    if INTERPRETERS.contains(&name)
        && let Some(script) = args.split_whitespace().nth(1)
    {
        let script = program(script);
        if LAUNCHERS.contains(&script) && !INTERPRETERS.contains(&script) {
            return script;
        }
    }
    name
}

/// `sh -c '…'`: what package managers run scripts with; part of the job.
/// A shell without `-c` is the user's terminal: where a job starts.
fn is_shell_command(args: &str) -> bool {
    SHELLS.contains(&program(args)) && args.split_whitespace().any(|a| a == "-c")
}

fn is_launcher(args: &str) -> bool {
    LAUNCHERS.contains(&tool(args)) || is_shell_command(args)
}

/// A command line made readable: paths shortened to file names, and an
/// interpreter in front of a launcher dropped (`pnpm dev`, not
/// `node /opt/homebrew/bin/pnpm dev`).
pub fn short_command(args: &str) -> String {
    let words: Vec<&str> = if is_shell_command(args) {
        // `sh -c vite --port 3000` → `vite --port 3000`
        args.split_whitespace()
            .skip_while(|w| *w != "-c")
            .skip(1)
            .collect()
    } else {
        args.split_whitespace().collect()
    };
    let interpreted = words.len() > 1 && INTERPRETERS.contains(&program(words[0]));
    // `node …/bin/pnpm dev` → `pnpm dev`
    let skip = usize::from(interpreted && tool(&words.join(" ")) != program(words[0]));
    // The program, and an interpreter's script, by file name; other
    // arguments (data directories, config files) as given.
    let shortened = if interpreted && skip == 0 { 2 } else { 1 };
    words[skip..]
        .iter()
        .enumerate()
        .map(|(ix, w)| match w.rsplit_once('/') {
            Some((_, name)) if ix < shortened => name,
            _ => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `args` with its program path replaced by `name` (lsof's process name),
/// for apps whose path has spaces (`/Applications/Google Chrome.app/…`),
/// which split the command line in the wrong places.
fn named_command(args: &str, name: &str) -> Option<String> {
    let needle = format!("/{name}");
    args.match_indices(&needle).find_map(|(ix, _)| {
        let end = ix + needle.len();
        let rest = &args[end..];
        (rest.is_empty() || rest.starts_with(' ')).then(|| format!("{name}{rest}"))
    })
}

/// Everything listening for one command the user started (`bun run dev`
/// with its children), or one standalone process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortJob {
    /// The process the user started (the launcher), or the listener itself.
    pub root: u32,
    /// The root and all its descendants: what "stop" signals.
    pub pids: Vec<u32>,
    /// What to call it: `npm run dev`, `postgres -D …`.
    pub label: String,
    /// The tool that started it (`npm`, `bun`, `pnpm`, `python`, …).
    pub tool: String,
    /// The root's full command line.
    pub cmdline: String,
    /// The processes actually listening, as (pid, short command).
    pub listeners: Vec<(u32, String)>,
    /// Ports ascending, one bind each (the widest).
    pub ports: Vec<(u16, BindAddr)>,
    /// The root's working directory (the project folder), when known.
    pub cwd: Option<String>,
    /// Started from a dev tool or a typical local server (as opposed to
    /// apps such as AirPlay or Spotify that also listen).
    pub dev: bool,
}

impl PortJob {
    /// Docker Desktop / OrbStack publish container ports; stopping them
    /// stops Docker, so they are never picked by "select all".
    pub fn is_docker(&self) -> bool {
        self.listeners.iter().any(|(_, c)| {
            c.starts_with("com.docker") || c.starts_with("docker") || c.starts_with("OrbStack")
        })
    }

    /// A database or web server running on its own (`postgres`, `redis-server`),
    /// not a dev server someone started with a tool.
    pub fn is_daemon(&self) -> bool {
        self.listeners.iter().all(|(pid, _)| *pid == self.root)
            && DEV_DAEMONS.contains(&program(&self.cmdline))
    }
}

/// Groups `listeners` by the command that started them, climbing from each
/// listener through launchers (see [`LAUNCHERS`]) until the user's shell, a
/// terminal, launchd, `exclude` (Sawit itself and its ancestors) or
/// anything unknown (so an editor or terminal app is never part of a job).
pub fn group_into_jobs(listeners: &[Listener], procs: &[Proc], exclude: &[u32]) -> Vec<PortJob> {
    use std::collections::HashMap;
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let args_of = |pid: u32, fallback: &str| {
        by_pid
            .get(&pid)
            .map(|p| p.args.clone())
            .unwrap_or_else(|| fallback.to_string())
    };
    let mut jobs: Vec<PortJob> = Vec::new();
    for l in listeners {
        let mut root = l.pid;
        while let Some(parent) = by_pid.get(&root).and_then(|p| by_pid.get(&p.ppid)) {
            if parent.pid <= 1 || exclude.contains(&parent.pid) || !is_launcher(&parent.args) {
                break;
            }
            root = parent.pid;
        }
        let job = match jobs.iter_mut().position(|j| j.root == root) {
            Some(ix) => &mut jobs[ix],
            None => {
                let cmdline = args_of(root, &l.command);
                let mut pids = vec![root];
                let mut ix = 0;
                while ix < pids.len() {
                    if let Some(kids) = children.get(&pids[ix]) {
                        pids.extend(kids.iter().filter(|k| !exclude.contains(k)));
                    }
                    ix += 1;
                }
                let launched = root != l.pid;
                let listener_args = args_of(l.pid, &l.command);
                // A standalone process is named the way lsof names it.
                let (label, tool_name) = match named_command(&cmdline, &l.command) {
                    Some(named) if !launched => (named, l.command.clone()),
                    _ => (short_command(&cmdline), tool(&cmdline).to_string()),
                };
                let docker = l.command.starts_with("com.docker")
                    || l.command.starts_with("docker")
                    || l.command.starts_with("OrbStack");
                let dev = launched
                    || docker
                    || LAUNCHERS.contains(&tool(&listener_args))
                    || DEV_DAEMONS.contains(&program(&listener_args));
                jobs.push(PortJob {
                    root,
                    pids,
                    label,
                    tool: tool_name,
                    cmdline,
                    listeners: vec![],
                    ports: vec![],
                    cwd: None,
                    dev,
                });
                jobs.last_mut().expect("pushed above")
            }
        };
        if !job.listeners.iter().any(|(pid, _)| *pid == l.pid) {
            let short = by_pid
                .get(&l.pid)
                .map(|p| {
                    named_command(&p.args, &l.command).unwrap_or_else(|| short_command(&p.args))
                })
                .unwrap_or_else(|| l.command.clone());
            job.listeners.push((l.pid, short));
        }
        match job.ports.iter_mut().find(|(port, _)| *port == l.port) {
            Some(entry) => {
                if l.addr.is_wildcard() && !entry.1.is_wildcard() {
                    entry.1 = l.addr;
                }
            }
            None => job.ports.push((l.port, l.addr)),
        }
    }
    for j in &mut jobs {
        j.ports.sort_by_key(|(port, _)| *port);
    }
    // Dev jobs first, then by lowest port.
    jobs.sort_by_key(|j| (!j.dev, j.ports.first().map(|(port, _)| *port), j.root));
    jobs
}

/// Parses `lsof -a -d cwd -p … -F pn` output into (pid, directory).
pub fn parse_lsof_cwd(output: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut pid = None;
    for line in output.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let (Some(n), Some(p)) = (line.strip_prefix('n'), pid) {
            out.push((p, n.to_string()));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortVerdict {
    Free,
    /// Someone holds the exact address we want: starting will fail.
    Conflict {
        same_addr: Vec<Listener>,
        shadowing: Vec<Listener>,
    },
    /// Overlapping binds of different specificity: starting may succeed but
    /// clients reach different servers depending on the address they use.
    Shadowing {
        listeners: Vec<Listener>,
    },
}

/// Judges whether binding `intended` addresses on `port` collides with `listeners`.
pub fn analyze(port: u16, intended: &[BindAddr], listeners: &[Listener]) -> PortVerdict {
    let mut same_addr = Vec::new();
    let mut shadowing = Vec::new();
    for l in listeners.iter().filter(|l| l.port == port) {
        let relevant: Vec<_> = intended.iter().filter(|a| a.overlaps(&l.addr)).collect();
        if relevant.iter().any(|a| **a == l.addr) {
            same_addr.push(l.clone());
        } else if relevant
            .iter()
            .any(|a| a.is_wildcard() || l.addr.is_wildcard())
        {
            shadowing.push(l.clone());
        }
    }
    match (same_addr.is_empty(), shadowing.is_empty()) {
        (true, true) => PortVerdict::Free,
        (true, false) => PortVerdict::Shadowing {
            listeners: shadowing,
        },
        (false, _) => PortVerdict::Conflict {
            same_addr,
            shadowing,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCKER: &str = "p14087\nccom.docker.backend\nf74\ntIPv6\nn*:5433\n";
    const MIXED: &str =
        "p58423\ncPython\nf3\ntIPv4\nn127.0.0.1:57360\nf4\ntIPv6\nn*:57360\nf5\ntIPv4\nn*:57360\n";
    const LOCAL: [BindAddr; 2] = [BindAddr::LoopbackV4, BindAddr::LoopbackV6];

    fn listener(addr: BindAddr, port: u16) -> Listener {
        Listener {
            pid: 1,
            command: "x".into(),
            addr,
            port,
        }
    }

    /// The trees `npm run dev`, `pnpm dev`, `bun run dev` and `bun server.js`
    /// make on macOS (captured from real runs), started from a terminal.
    const PS: &str = "\
    1     0 /sbin/launchd
  500     1 /Applications/Ghostty.app/Contents/MacOS/ghostty
  501   500 /usr/bin/login -flp me /bin/zsh
  502   501 -zsh
  610   502 npm run dev
  611   610 sh -c node server.js
  612   611 node server.js
  620   502 node /opt/homebrew/bin/pnpm dev
  621   620 node server.js
  630   502 bun run dev
  631   630 node server.js
  640   502 bun server.js
  700   502 /opt/homebrew/opt/postgresql@17/bin/postgres -D /usr/local/var/postgres
  800     1 /System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter
  900     1 /Applications/Visual Studio Code.app/Contents/MacOS/Electron
  901   900 /bin/zsh -c npm run start
  902   901 npm run start
  903   902 node dist/main.js --port 4000
  950     1 /Applications/Docker.app/Contents/MacOS/com.docker.backend
";

    fn listen(pid: u32, command: &str, port: u16) -> Listener {
        Listener {
            pid,
            command: command.into(),
            addr: BindAddr::LoopbackV4,
            port,
        }
    }

    fn jobs() -> Vec<PortJob> {
        let ls = [
            listen(612, "node", 3000),
            listen(612, "node", 24678), // Vite's HMR socket: same job
            listen(621, "node", 3001),
            listen(631, "node", 3002),
            listen(640, "bun", 3003),
            listen(700, "postgres", 5432),
            listen(800, "ControlCenter", 7000),
            listen(903, "node", 4000),
            listen(950, "com.docker.backend", 6379),
        ];
        group_into_jobs(&ls, &parse_ps(PS), &[])
    }

    fn job<'a>(jobs: &'a [PortJob], label: &str) -> &'a PortJob {
        jobs.iter().find(|j| j.label == label).unwrap_or_else(|| {
            panic!(
                "no job {label}: {:#?}",
                jobs.iter().map(|j| &j.label).collect::<Vec<_>>()
            )
        })
    }

    #[test]
    fn listeners_are_grouped_under_the_command_the_user_ran() {
        let jobs = jobs();
        let npm = job(&jobs, "npm run dev");
        assert_eq!((npm.root, npm.tool.as_str()), (610, "npm"));
        assert_eq!(npm.pids, vec![610, 611, 612]);
        assert_eq!(npm.listeners, vec![(612, "node server.js".to_string())]);
        assert_eq!(
            npm.ports,
            vec![(3000, BindAddr::LoopbackV4), (24678, BindAddr::LoopbackV4)]
        );
        assert!(npm.dev);
        let pnpm = job(&jobs, "pnpm dev");
        assert_eq!((pnpm.root, pnpm.tool.as_str()), (620, "pnpm"));
        let bun = job(&jobs, "bun run dev");
        assert_eq!(
            (bun.root, bun.tool.as_str(), bun.pids.clone()),
            (630, "bun", vec![630, 631])
        );
        let bun_direct = job(&jobs, "bun server.js");
        assert_eq!((bun_direct.root, bun_direct.tool.as_str()), (640, "bun"));
        // The terminal's shell, login and the terminal app stay out.
        assert!(
            jobs.iter()
                .all(|j| !j.pids.contains(&502) && !j.pids.contains(&500))
        );
    }

    #[test]
    fn an_editor_that_runs_a_task_is_never_part_of_the_job() {
        let jobs = jobs();
        // VS Code ran `zsh -c npm run start`: the job starts at that shell.
        let task = job(&jobs, "npm run start");
        assert_eq!(task.root, 901);
        assert_eq!(task.pids, vec![901, 902, 903]);
    }

    #[test]
    fn standalone_servers_apps_and_docker() {
        let jobs = jobs();
        let pg = jobs.iter().find(|j| j.root == 700).unwrap();
        assert_eq!(pg.tool, "postgres");
        assert_eq!(pg.label, "postgres -D /usr/local/var/postgres");
        assert!(pg.dev);
        let airplay = jobs.iter().find(|j| j.root == 800).unwrap();
        assert!(!airplay.dev);
        assert_eq!(airplay.label, "ControlCenter");
        let docker = jobs.iter().find(|j| j.root == 950).unwrap();
        assert!(docker.is_docker() && docker.dev);
        assert_eq!(docker.label, "com.docker.backend");
        // Dev jobs are listed first.
        assert!(!jobs.last().unwrap().dev);
    }

    #[test]
    fn apps_with_spaces_in_their_path_keep_their_name() {
        let ps = "  70     1 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --remote-debugging-port=9222\n";
        let jobs = group_into_jobs(&[listen(70, "Google Chrome", 9222)], &parse_ps(ps), &[]);
        assert_eq!(jobs[0].label, "Google Chrome --remote-debugging-port=9222");
        assert_eq!(jobs[0].tool, "Google Chrome");
        assert!(!jobs[0].dev);
    }

    #[test]
    fn sawit_and_its_ancestors_are_never_climbed_into() {
        let ps = "  10     1 cargo run\n  11    10 /x/target/debug/sawit\n  12    11 node srv.js\n";
        let jobs = group_into_jobs(&[listen(12, "node", 3000)], &parse_ps(ps), &[11, 10]);
        assert_eq!((jobs[0].root, jobs[0].pids.clone()), (12, vec![12]));
    }

    #[test]
    fn tools_and_short_commands() {
        assert_eq!(tool("node /opt/homebrew/bin/pnpm dev"), "pnpm");
        assert_eq!(
            tool("node /usr/local/lib/node_modules/yarn/bin/yarn.js dev"),
            "node"
        );
        assert_eq!(tool("/usr/bin/python3.12 manage.py runserver"), "python");
        assert_eq!(tool("-zsh"), "zsh");
        assert_eq!(tool("node22 x.js"), "node");
        assert_eq!(short_command("node /opt/homebrew/bin/pnpm dev"), "pnpm dev");
        assert_eq!(short_command("sh -c vite --port 3000"), "vite --port 3000");
        assert_eq!(
            short_command("node /app/node_modules/.bin/next dev"),
            "next dev"
        );
        assert_eq!(
            short_command("/usr/bin/python3 manage.py runserver"),
            "python3 manage.py runserver"
        );
    }

    #[test]
    fn parses_ps_and_cwd_output() {
        let procs =
            parse_ps("  610   502 npm run dev   \n bad line\n  611   610 sh -c node server.js\n");
        assert_eq!(
            procs,
            vec![
                Proc {
                    pid: 610,
                    ppid: 502,
                    args: "npm run dev".into()
                },
                Proc {
                    pid: 611,
                    ppid: 610,
                    args: "sh -c node server.js".into()
                },
            ]
        );
        assert_eq!(
            parse_lsof_cwd("p610\nfcwd\nn/Users/me/app\np700\nfcwd\nn/data\n"),
            vec![(610, "/Users/me/app".into()), (700, "/data".into())]
        );
    }

    #[test]
    fn parses_docker_v6_wildcard() {
        assert_eq!(
            parse_lsof(DOCKER),
            vec![Listener {
                pid: 14087,
                command: "com.docker.backend".into(),
                addr: BindAddr::AnyV6,
                port: 5433
            }]
        );
    }

    #[test]
    fn parses_mixed_fds_of_one_process() {
        let addrs: Vec<_> = parse_lsof(MIXED)
            .into_iter()
            .map(|l| (l.pid, l.addr, l.port))
            .collect();
        assert_eq!(
            addrs,
            vec![
                (58423, BindAddr::LoopbackV4, 57360),
                (58423, BindAddr::AnyV6, 57360),
                (58423, BindAddr::AnyV4, 57360),
            ]
        );
    }

    #[test]
    fn parses_ipv6_loopback_brackets() {
        let l = parse_lsof("p9\ncpostgres\nf7\ntIPv6\nn[::1]:5432\n");
        assert_eq!(l[0].addr, BindAddr::LoopbackV6);
        assert_eq!(l[0].port, 5432);
    }

    #[test]
    fn parses_specific_address() {
        let l = parse_lsof("p9\ncpostgres\nf7\ntIPv4\nn192.168.1.5:5432\n");
        assert_eq!(
            l[0].addr,
            BindAddr::Specific("192.168.1.5".parse().unwrap())
        );
    }

    #[test]
    fn empty_lsof_output_is_free() {
        assert_eq!(
            analyze(5433, &[BindAddr::LoopbackV4], &parse_lsof("")),
            PortVerdict::Free
        );
    }

    #[test]
    fn docker_v6_wildcard_shadows_loopback_v4() {
        let verdict = analyze(5433, &LOCAL, &parse_lsof(DOCKER));
        assert!(
            matches!(verdict, PortVerdict::Shadowing { ref listeners } if listeners.len() == 1)
        );
    }

    #[test]
    fn same_loopback_is_conflict() {
        let verdict = analyze(
            5433,
            &[BindAddr::LoopbackV4],
            &[listener(BindAddr::LoopbackV4, 5433)],
        );
        assert!(
            matches!(verdict, PortVerdict::Conflict { ref same_addr, .. } if same_addr.len() == 1)
        );
    }

    #[test]
    fn wildcard_intent_vs_loopback_listener_shadows() {
        let verdict = analyze(
            5433,
            &[BindAddr::AnyV4],
            &[listener(BindAddr::LoopbackV4, 5433)],
        );
        assert!(matches!(verdict, PortVerdict::Shadowing { .. }));
    }

    #[test]
    fn v6_loopback_listener_ignored_for_v4_only_intent() {
        let verdict = analyze(
            5433,
            &[BindAddr::LoopbackV4],
            &[listener(BindAddr::LoopbackV6, 5433)],
        );
        assert_eq!(verdict, PortVerdict::Free);
    }

    #[test]
    fn other_port_ignored() {
        let verdict = analyze(5433, &LOCAL, &[listener(BindAddr::AnyV4, 5432)]);
        assert_eq!(verdict, PortVerdict::Free);
    }

    #[test]
    fn conflict_also_reports_shadowing_listeners() {
        let ls = [
            listener(BindAddr::LoopbackV4, 5433),
            listener(BindAddr::AnyV4, 5433),
        ];
        match analyze(5433, &[BindAddr::LoopbackV4], &ls) {
            PortVerdict::Conflict {
                same_addr,
                shadowing,
            } => {
                assert_eq!(same_addr.len(), 1);
                assert_eq!(shadowing.len(), 1);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }
}
