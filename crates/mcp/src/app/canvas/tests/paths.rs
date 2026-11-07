//! B4: every MCP path the canvas's coverage table names is a tool this server has.

use sawit_board::mcp_paths::{APP_CONTROLS, McpPath, mcp_path, person_reachable};

use crate::Sawit;

#[test]
fn every_path_names_a_tool_that_exists() {
    let tools: Vec<String> = Sawit::router()
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    let board = sawit_board::BoardDoc::new_board("Board").unwrap();
    let named = person_reachable(&[&board.session])
        .into_iter()
        .filter_map(mcp_path)
        .chain(APP_CONTROLS.iter().map(|(_, path)| *path));
    for path in named {
        if let McpPath::Tool(tool, _) = path {
            assert!(tools.iter().any(|t| t == tool), "{tool} is no tool");
        }
    }
}
