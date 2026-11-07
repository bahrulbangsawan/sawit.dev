use super::*;

const NOW: f64 = 1_000_000.0;

fn acct(name: &str, provider: Provider, limits: &[(LimitKind, f64, f64)]) -> PoolAccount {
    PoolAccount {
        id: name.into(),
        provider,
        name: name.into(),
        color: "#D97757".into(),
        plan: None,
        signed_in: Some(true),
        limits: limits
            .iter()
            .map(|&(kind, used_pct, resets_in)| Limit {
                kind,
                used_pct,
                resets_at: NOW + resets_in,
            })
            .collect(),
        observed_at: Some(NOW - 60.0),
        refresh_error: None,
        active: false,
    }
}

#[test]
fn statuses_follow_the_tightest_limit() {
    use LimitKind::*;
    let s = |l: &[(LimitKind, f64, f64)]| status(&acct("a", Provider::ClaudeCode, l), NOW);
    assert_eq!(
        s(&[(FiveHour, 20.0, 3600.0), (Weekly, 40.0, 86400.0)]),
        Status::PlentyLeft
    );
    assert_eq!(s(&[(FiveHour, 80.0, 3600.0)]), Status::RunningLow);
    assert_eq!(s(&[(FiveHour, 95.0, 3600.0)]), Status::AlmostOut);
    assert_eq!(s(&[(Weekly, 100.0, 3600.0)]), Status::LimitReached);
    // A window that reset since the reading has room again.
    assert_eq!(s(&[(FiveHour, 100.0, -10.0)]), Status::PlentyLeft);
    assert_eq!(s(&[]), Status::NotReported);
    let mut old = acct("a", Provider::ClaudeCode, &[(FiveHour, 20.0, 3600.0)]);
    old.observed_at = Some(NOW - 2.0 * STALE_SECS);
    assert_eq!(status(&old, NOW), Status::OutOfDate);
    old.limits[0].used_pct = 100.0;
    assert_eq!(
        status(&old, NOW),
        Status::LimitReached,
        "reached stays reached"
    );
    old.signed_in = Some(false);
    assert_eq!(status(&old, NOW), Status::SignInExpired);
    let mut failed = acct("a", Provider::Codex, &[]);
    failed.refresh_error = Some("unreadable".into());
    assert_eq!(status(&failed, NOW), Status::RefreshFailed);
}

#[test]
fn the_pick_has_the_most_room_and_says_why() {
    use LimitKind::*;
    let mut out = acct("out", Provider::ClaudeCode, &[(FiveHour, 10.0, 3600.0)]);
    out.signed_in = Some(false);
    let all = vec![
        acct(
            "work",
            Provider::ClaudeCode,
            &[(FiveHour, 30.0, 3600.0), (Weekly, 70.0, 86400.0)],
        ),
        acct(
            "side",
            Provider::ClaudeCode,
            &[(FiveHour, 50.0, 3600.0), (Weekly, 45.0, 86400.0)],
        ),
        acct("codex", Provider::Codex, &[]),
        out,
    ];
    let r = recommend(&all, None, NOW).unwrap();
    assert_eq!(r.account, "side");
    assert_eq!(r.room, 50.0);
    assert_eq!(r.holding, Some(FiveHour));
    assert!(r.reason.contains("50%"), "{}", r.reason);
    assert_eq!(r.next, Some(("work".into(), 30.0)));
    assert_eq!(r.skipped.len(), 2);
    assert!(recommend(&all, Some(Provider::Codex), NOW).is_none());
}

#[test]
fn readings_make_windows_with_peaks_and_hits() {
    let r = |t: f64, used: f64, resets: f64| Reading {
        t,
        kind: LimitKind::FiveHour,
        used_pct: used,
        resets_at: resets,
    };
    let readings = [
        r(100.0, 10.0, 18_000.0),
        r(200.0, 60.0, 18_030.0),
        r(300.0, 100.0, 18_000.0),
        r(20_000.0, 5.0, 38_000.0),
    ];
    let spans = windows(&readings, LimitKind::FiveHour);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].peak_pct, 100.0);
    assert!(spans[0].hit_limit);
    assert_eq!(spans[0].hit_at, Some(300.0));
    assert_eq!(spans[0].start, 0.0);
    assert!(!spans[1].hit_limit);
    assert_eq!(resets_between(&spans, 0.0, 20_000.0).len(), 1);
}

#[test]
fn a_fast_pace_runs_out_before_the_reset() {
    // Half the 5-hour window gone, 80% used: 1.6 times an even pace.
    let l = Limit {
        kind: LimitKind::FiveHour,
        used_pct: 80.0,
        resets_at: NOW + 9000.0,
    };
    let f = forecast(&l, NOW).unwrap();
    assert!((f.elapsed_pct - 50.0).abs() < 1e-9);
    assert!((f.pace - 1.6).abs() < 1e-9);
    assert!(f.projected_pct > 100.0);
    let out = f.runs_out_at.unwrap();
    assert!(out > NOW && out < l.resets_at);
    let slow = Limit {
        used_pct: 20.0,
        ..l
    };
    assert_eq!(forecast(&slow, NOW).unwrap().runs_out_at, None);
    let fresh = Limit {
        resets_at: NOW + 17_990.0,
        ..l
    };
    assert!(forecast(&fresh, NOW).is_none(), "too early to judge");
}

#[test]
fn the_heatmap_counts_rises_by_weekday_and_hour() {
    // 1970-01-05 was a Monday.
    let monday_9 = 4.0 * 86400.0 + 9.0 * 3600.0;
    let r = |t: f64, used: f64| Reading {
        t,
        kind: LimitKind::FiveHour,
        used_pct: used,
        resets_at: monday_9 + 18_000.0,
    };
    let map = heatmap(
        &[
            r(monday_9, 10.0),
            r(monday_9 + 600.0, 30.0),
            r(monday_9 + 3700.0, 100.0),
        ],
        LimitKind::FiveHour,
        &|_| 0,
    );
    assert_eq!(map.used[0][9], 30.0);
    assert_eq!(map.used[0][10], 70.0);
    assert_eq!(map.locked[0][10], 1);
    assert_eq!(map.busiest(), Some((0, 10, 70.0)));
}

#[test]
fn plan_fit_reads_the_weekly_windows() {
    let w = |peak: f64| WindowSpan {
        kind: LimitKind::Weekly,
        start: 0.0,
        end: 1.0,
        peak_pct: peak,
        hit_limit: peak >= 100.0,
        hit_at: None,
    };
    assert_eq!(plan_fit(&[w(50.0)], None).verdict, FitVerdict::TooEarly);
    assert_eq!(
        plan_fit(&[w(100.0), w(100.0), w(40.0)], None).verdict,
        FitVerdict::TooSmall
    );
    assert_eq!(
        plan_fit(&[w(5.0), w(8.0)], None).verdict,
        FitVerdict::CouldCancel
    );
    let fit = plan_fit(&[w(20.0), w(30.0)], Some(100.0));
    assert_eq!(fit.verdict, FitVerdict::CouldDownsize);
    assert_eq!(fit.unused_per_month, Some(75.0));
    assert_eq!(
        plan_fit(&[w(60.0), w(80.0)], None).verdict,
        FitVerdict::Fits
    );
}

#[test]
fn work_moves_to_an_account_of_the_same_agent_with_room() {
    use LimitKind::*;
    let all = vec![
        acct("work", Provider::ClaudeCode, &[(FiveHour, 95.0, 3600.0)]),
        acct("side", Provider::ClaudeCode, &[(FiveHour, 10.0, 3600.0)]),
        acct("codex", Provider::Codex, &[(FiveHour, 0.0, 3600.0)]),
    ];
    let moves = move_work(&all, NOW);
    assert_eq!(moves.len(), 1);
    assert_eq!(
        (moves[0].from.as_str(), moves[0].to.as_str()),
        ("work", "side")
    );
    let json = overview_json(&all, NOW);
    assert_eq!(json["recommended"]["account"], "codex");
    assert_eq!(json["accounts"][0]["status"], "almost-out");
}
