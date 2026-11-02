//! A project's data store, its dev servers, and the projects in the app's state.

use std::path::Path;

use serde_json::json;

use crate::browser::{
    data_store_id, dev_server_url, dev_servers, front_project, pick_dev_server, project_holding,
};
use crate::ports::PortJob;

#[test]
fn store_ids_are_stable_per_folder() {
    let a = data_store_id(Path::new("/code/shop"));
    assert_eq!(a, data_store_id(Path::new("/code/shop")));
    assert_ne!(a, data_store_id(Path::new("/code/blog")));
    assert_eq!(a[6] >> 4, 4);
}

fn job(cwd: &str, port: u16) -> PortJob {
    PortJob {
        root: 1,
        pids: vec![1],
        label: "bun run dev".into(),
        tool: "bun".into(),
        cmdline: "bun run dev".into(),
        listeners: vec![(2, "node".into())],
        ports: vec![(port, crate::ports::BindAddr::LoopbackV4)],
        cwd: Some(cwd.into()),
        dev: true,
    }
}

#[test]
fn dev_server_of_the_project() {
    let project = Path::new("/code/shop");
    assert_eq!(
        pick_dev_server(
            &[job("/code/shop/web", 5173), job("/code/blog", 3000)],
            project
        ),
        Some("http://localhost:5173".into())
    );
    assert_eq!(
        pick_dev_server(
            &[job("/code/shop", 5173), job("/code/shop/api", 3000)],
            project
        ),
        None
    );
    assert_eq!(pick_dev_server(&[], project), None);
}

#[test]
fn dev_servers_in_a_folder() {
    let mut api = job("/code/shop/api", 3000);
    api.ports.push((3001, crate::ports::BindAddr::LoopbackV4));
    let mut database = job("/code/shop", 5432);
    database.dev = false;
    let jobs = [job("/code/shop/web", 5173), api, database];
    let found = |folder: &str| -> Vec<u16> {
        dev_servers(&jobs, Path::new(folder))
            .iter()
            .map(|j| j.ports[0].0)
            .collect()
    };
    assert_eq!(found("/code/shop"), [5173, 3000]);
    assert_eq!(found("/code/shop/api"), [3000]);
    assert_eq!(found("/code/blog"), Vec::<u16>::new());
    // The lowest port of a server with several.
    assert_eq!(
        dev_server_url(dev_servers(&jobs, Path::new("/code/shop/api"))[0]),
        Some("http://localhost:3000".into())
    );
}

/// What `sawit browse` reads of the app's state: the front window's project, and the open
/// project that holds a folder, the deepest of any window's.
#[test]
fn projects_in_the_apps_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (shop, api) = (root.join("shop"), root.join("shop").join("api"));
    std::fs::create_dir_all(api.join("src")).unwrap();
    let state = json!({"windows": [
        {"project": shop, "projects": [shop], "frontmost": false},
        {"project": api, "projects": [root.join("blog"), api], "frontmost": true},
    ]});
    assert_eq!(front_project(&state), Some(api.clone()));
    assert_eq!(project_holding(&state, &api.join("src")), Some(api.clone()));
    assert_eq!(project_holding(&state, &shop), Some(shop.clone()));
    assert_eq!(project_holding(&state, &root), None);
    // A folder named through a link is the project the link leads to.
    let link = root.join("link");
    std::os::unix::fs::symlink(&shop, &link).unwrap();
    assert_eq!(project_holding(&state, &link), Some(shop));
    assert_eq!(front_project(&json!({"windows": []})), None);
    assert_eq!(project_holding(&json!({}), &api), None);
}
