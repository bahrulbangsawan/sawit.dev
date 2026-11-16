use super::*;

#[test]
fn cli_socket_stays_under_the_unix_path_limit() {
    let short = AppPaths::resolve(None, Some("/Users/me".into()));
    assert_eq!(
        short.cli_socket(),
        PathBuf::from("/Users/me/Library/Application Support/sawit/sawit.sock")
    );
    let deep = AppPaths::resolve(Some("/x".repeat(60).into()), None);
    let socket = deep.cli_socket();
    assert!(socket.starts_with("/tmp") && socket.as_os_str().len() < 104);
    assert_eq!(socket, deep.cli_socket());
}

fn inst(name: &str, port: u16) -> Instance {
    Instance {
        name: name.into(),
        bin_dir: "/opt/homebrew/opt/postgresql@16/bin".into(),
        data_dir: format!("/data/{name}").into(),
        port,
        listen: ListenScope::Localhost,
        managed: true,
    }
}

#[test]
fn update_refuses_an_unreadable_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("instances.toml");
    std::fs::write(&file, "not = [toml").unwrap();
    let out = Registry::update(&file, |r| r.add(inst("a", 5433)));
    assert!(out.is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not = [toml");
}

#[test]
fn update_keeps_what_another_writer_saved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("instances.toml");
    let mut mine = Registry::default();
    mine.add(inst("a", 5433)).unwrap();
    mine.save(&file).unwrap();
    let stale = Registry::load(&file).unwrap();
    let mut other = stale.clone();
    other.add(inst("b", 5434)).unwrap();
    other.save(&file).unwrap();
    let (now, ()) = Registry::update(&file, |r| r.add(inst("c", 5435))).unwrap();
    assert_eq!(now.instances.len(), 3);
    assert_eq!(Registry::load(&file).unwrap(), now);
}

#[test]
fn roundtrip_toml() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sub/instances.toml");
    let mut reg = Registry::default();
    reg.add(inst("app", 5440)).unwrap();
    reg.add(Instance {
        listen: ListenScope::All,
        ..inst("crm", 5441)
    })
    .unwrap();
    reg.save(&file).unwrap();
    assert_eq!(Registry::load(&file).unwrap(), reg);
}

#[test]
fn missing_file_loads_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Registry::load(&dir.path().join("nope.toml"))
            .unwrap()
            .instances
            .is_empty()
    );
}

#[test]
fn duplicate_name_rejected() {
    let mut reg = Registry::default();
    reg.add(inst("app", 5440)).unwrap();
    assert!(
        reg.add(Instance {
            data_dir: "/other".into(),
            ..inst("app", 5441)
        })
        .is_err()
    );
}

#[test]
fn duplicate_data_dir_rejected() {
    let mut reg = Registry::default();
    reg.add(inst("app", 5440)).unwrap();
    assert!(
        reg.add(Instance {
            data_dir: "/data/app".into(),
            ..inst("app2", 5441)
        })
        .is_err()
    );
}

#[test]
fn same_port_listed_in_port_clashes() {
    let mut reg = Registry::default();
    reg.add(inst("a", 5433)).unwrap();
    reg.add(inst("b", 5433)).unwrap();
    reg.add(inst("c", 5434)).unwrap();
    assert_eq!(
        reg.port_clashes(),
        vec![("a".to_string(), "b".to_string(), 5433)]
    );
}

#[test]
fn remove_and_get() {
    let mut reg = Registry::default();
    reg.add(inst("a", 5433)).unwrap();
    assert!(reg.get("a").is_some());
    reg.remove("a");
    assert!(reg.get("a").is_none());
}

#[test]
fn invalid_names_rejected() {
    for bad in ["My DB", "", &"x".repeat(41), "a/b", "../x"] {
        assert!(validate_name(bad).is_err(), "{bad:?}");
    }
    for good in ["app", "kyzn-crm_2", &"x".repeat(40)] {
        assert!(validate_name(good).is_ok(), "{good:?}");
    }
}

#[test]
fn socket_path_too_long_is_rejected() {
    let long = std::path::PathBuf::from(format!("/{}", "d".repeat(99)));
    assert!(validate_socket_dir(&long, 5432).is_err());
    assert!(
        validate_socket_dir(
            Path::new("/Users/someone/Library/Application Support/sawit/run"),
            5432
        )
        .is_ok()
    );
}

#[test]
fn listen_scope_addresses() {
    assert_eq!(
        ListenScope::Localhost.bind_addrs(),
        vec![BindAddr::LoopbackV4, BindAddr::LoopbackV6]
    );
    assert_eq!(ListenScope::All.listen_addresses(), "*");
}

#[test]
fn conf_port_ignores_comments() {
    assert_eq!(
        parse_conf_port("#port = 5432\nport = 5440 # x\n"),
        Some(5440)
    );
    assert_eq!(parse_conf_port("#port = 5432\n"), None);
    assert_eq!(parse_conf_port("port = 'abc'\n"), None);
}

#[test]
fn app_paths_resolve_override_then_home() {
    let paths = AppPaths::resolve(Some("/custom".into()), Some("/Users/u".into()));
    assert_eq!(paths.root, Path::new("/custom"));
    let paths = AppPaths::resolve(None, Some("/Users/u".into()));
    assert_eq!(
        paths.root,
        Path::new("/Users/u/Library/Application Support/sawit")
    );
    let paths = AppPaths::resolve(Some("".into()), Some("/Users/u".into()));
    assert_eq!(
        paths.root,
        Path::new("/Users/u/Library/Application Support/sawit")
    );
}

#[test]
fn legacy_root_moves_once_and_keeps_its_paths() {
    let dir = tempfile::tempdir().unwrap();
    let (legacy, root) = (dir.path().join("pgdesk"), dir.path().join("sawit"));
    std::fs::create_dir_all(legacy.join("clusters/app")).unwrap();
    let toml = |r: &Path| {
        format!(
            "[[instance]]\nname = \"app\"\nbin_dir = \"/opt/pg/bin\"\ndata_dir = \"{}/clusters/app\"\nport = 5440\nlisten = \"localhost\"\nmanaged = true\n",
            r.display()
        )
    };
    std::fs::write(legacy.join("instances.toml"), toml(&legacy)).unwrap();

    migrate_legacy_root(&legacy, &root);
    assert!(!legacy.exists());
    assert!(root.join("clusters/app").is_dir());
    let reg = Registry::load(&root.join("instances.toml")).unwrap();
    assert_eq!(reg.instances[0].data_dir, root.join("clusters/app"));
    assert_eq!(reg.instances[0].bin_dir, Path::new("/opt/pg/bin"));

    // Once `root` exists, a leftover legacy dir is left alone.
    std::fs::create_dir_all(&legacy).unwrap();
    migrate_legacy_root(&legacy, &root);
    assert!(legacy.is_dir());
    assert_eq!(
        std::fs::read_to_string(root.join("instances.toml")).unwrap(),
        toml(&root)
    );
}

#[test]
fn app_paths_layout() {
    let p = AppPaths { root: "/r".into() };
    assert_eq!(p.cluster_dir("app"), Path::new("/r/clusters/app"));
    assert_eq!(p.log_file("app"), Path::new("/r/logs/app.log"));
    assert_eq!(p.socket_dir(), Path::new("/r/run"));
    assert_eq!(p.managed_bin_root(), Path::new("/r/bin"));
    assert_eq!(p.instances_file(), Path::new("/r/instances.toml"));
}
