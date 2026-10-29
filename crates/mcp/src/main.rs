//! `sawit-mcp`: stdio MCP server. Register it in an agent, e.g.
//! `claude mcp add sawit -- /path/to/sawit-mcp`. The app bundle has no copy
//! of this binary: its `sawit-mcp` is a symlink to `sawit`, which runs
//! [`sawit_mcp::main`] when started under that name.

fn main() -> std::process::ExitCode {
    sawit_mcp::main()
}
