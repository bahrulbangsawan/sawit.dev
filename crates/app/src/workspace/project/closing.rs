//! The question before a project closes (backlog items 12 and 13): what would be lost with it
//! (unsaved files, the browser's notes not yet sent) and what would stop (its terminals' running
//! processes, its Run panel scripts among them). Nothing of that: no question.

use crate::confirm::Confirm;

/// What closing a project would lose or stop.
#[derive(Clone, Debug, Default)]
pub(crate) struct Losing {
    pub unsaved: usize,
    pub notes: usize,
    /// What runs in its terminals, one name per busy pane.
    pub running: Vec<String>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The question before closing project `name`, or `None` when nothing would be lost or stopped.
pub(crate) fn question(name: &str, losing: &Losing) -> Option<Confirm> {
    let Losing {
        unsaved,
        notes,
        running,
    } = losing;
    let mut lost = Vec::new();
    if *unsaved > 0 {
        lost.push(plural(*unsaved, "unsaved file", "unsaved files"));
    }
    if *notes > 0 {
        lost.push("its browser's annotations".to_string());
    }
    let (title, verb) = match (running.len(), lost.is_empty()) {
        (0, true) => return None,
        (0, false) => (
            format!("Close {name} with {}?", lost.join(" and ")),
            if *unsaved > 0 {
                "Close and Discard Changes"
            } else {
                "Close and Discard Notes"
            },
        ),
        (n, _) => (
            format!(
                "Close {name} and stop {}?",
                plural(n, "running process", "running processes")
            ),
            "Close and Stop",
        ),
    };
    let mut detail = String::new();
    if *unsaved > 0 {
        detail.push_str("Unsaved changes are lost. ");
    }
    if *notes > 0 {
        detail.push_str(&crate::browser::notes_lost(*notes));
        detail.push(' ');
    }
    if !running.is_empty() {
        detail.push_str(
            "Its terminals close, and everything they started is stopped (SIGHUP, then SIGTERM, \
             then SIGKILL), its dev servers' ports with them.",
        );
    }
    Some(
        Confirm::new(title, verb)
            .thing(name.to_string())
            .detail(detail.trim().to_string())
            .items(running.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_what_stops_and_what_is_lost() {
        assert!(question("web", &Losing::default()).is_none());
        let q = question(
            "web",
            &Losing {
                unsaved: 2,
                notes: 0,
                running: vec!["bun dev".into(), "claude".into(), "prod (SSH)".into()],
            },
        )
        .unwrap();
        assert_eq!(q.title.as_ref(), "Close web and stop 3 running processes?");
        assert_eq!(q.verb.as_ref(), "Close and Stop");
        assert_eq!(q.listed().len(), 3);
        let detail = q.detail.clone().unwrap();
        assert!(detail.starts_with("Unsaved changes are lost."), "{detail}");
        let q = question(
            "web",
            &Losing {
                unsaved: 1,
                ..Losing::default()
            },
        )
        .unwrap();
        assert_eq!(q.title.as_ref(), "Close web with 1 unsaved file?");
        assert_eq!(q.verb.as_ref(), "Close and Discard Changes");
    }
}
