//! Command lines: the tool one runs (`pnpm`, not the `node` running it), whether it launches
//! others, and how to show it.

/// Package managers, runtimes, task runners and dev servers: what sits
/// between a terminal and the process that opens a port. Climbing the
/// process tree through these finds the command the user typed.
pub(super) const LAUNCHERS: &[&str] = &[
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
pub(super) const DEV_DAEMONS: &[&str] = &[
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
pub(super) fn program(args: &str) -> &str {
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
/// launcher. A bare word after it is a subcommand (`deno task dev`), not
/// a script.
pub fn tool(args: &str) -> &str {
    let name = program(args);
    if INTERPRETERS.contains(&name)
        && let Some(script) = args.split_whitespace().nth(1)
        && script.contains('/')
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

pub(super) fn is_launcher(args: &str) -> bool {
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
pub(super) fn named_command(args: &str, name: &str) -> Option<String> {
    let needle = format!("/{name}");
    args.match_indices(&needle).find_map(|(ix, _)| {
        let end = ix + needle.len();
        let rest = &args[end..];
        (rest.is_empty() || rest.starts_with(' ')).then(|| format!("{name}{rest}"))
    })
}
