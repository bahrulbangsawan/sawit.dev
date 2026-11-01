//! Tests for ports: reading `lsof` and `ps`, grouping listeners into jobs, and conflicts.

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
    // `task` is also go-task's name; here it is deno's subcommand.
    assert_eq!(tool("deno task dev"), "deno");
    assert_eq!(short_command("deno task dev"), "deno task dev");
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
    assert!(matches!(verdict, PortVerdict::Shadowing { ref listeners } if listeners.len() == 1));
}

#[test]
fn same_loopback_is_conflict() {
    let verdict = analyze(
        5433,
        &[BindAddr::LoopbackV4],
        &[listener(BindAddr::LoopbackV4, 5433)],
    );
    assert!(matches!(verdict, PortVerdict::Conflict { ref same_addr, .. } if same_addr.len() == 1));
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
