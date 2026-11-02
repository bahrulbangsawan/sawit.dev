//! Tests of the command line: targets, flags, subcommands and their help, the open request and
//! the `sawit` link.

use super::*;

mod help;
mod link;
mod paths;
mod request;
mod subcommands;

fn target(raw: &str, existing: &[&str]) -> Target {
    parse_target(OsStr::new(raw), Path::new("/p"), |p| {
        existing.iter().any(|e| p == Path::new(e))
    })
}

fn at(path: &str, line: Option<u32>, column: Option<u32>) -> Target {
    Target {
        path: path.into(),
        line,
        column,
    }
}

fn parse(args: &[&str]) -> std::result::Result<Invocation, String> {
    parse_args(args.iter().map(OsString::from), Path::new("/nope"))
}
