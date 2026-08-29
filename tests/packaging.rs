//! Guards for files that ship inside distro packages and can silently rot:
//! nothing in the build fails when a unit file keeps a placeholder or the
//! generated shell completions drift from the CLI.

/// The `.deb`/`.rpm` install a *rendered* unit — shipping the template would
/// give every package user an `ExecStart=__BIN__ daemon run` that cannot
/// start. Both files must otherwise stay identical.
#[test]
fn packaged_systemd_unit_matches_the_template() {
    let template = include_str!("../assets/systemd/monk.service");
    let packaged = include_str!("../assets/systemd/monk-packaged.service");

    assert!(
        template.contains("ExecStart=__BIN__ daemon run"),
        "the runtime template must keep the placeholder that install_service fills in"
    );
    assert!(
        packaged.contains("ExecStart=/usr/bin/monk daemon run"),
        "the packaged unit must point at the packaged binary"
    );
    assert!(!packaged.contains("__BIN__"), "the packaged unit must not ship a placeholder");

    let strip = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter(|l| !l.starts_with("ExecStart="))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        strip(template),
        strip(packaged),
        "monk.service and monk-packaged.service drifted — keep every line but ExecStart in sync"
    );
}

/// The checked-in completions are what `.deb`/`.rpm` install; regenerate them
/// with `just completions` whenever the command tree changes.
#[test]
fn shipped_completions_match_the_current_cli() {
    for (shell, path, bytes) in [
        (
            clap_complete::Shell::Bash,
            "assets/completions/monk",
            include_str!("../assets/completions/monk"),
        ),
        (
            clap_complete::Shell::Zsh,
            "assets/completions/_monk",
            include_str!("../assets/completions/_monk"),
        ),
        (
            clap_complete::Shell::Fish,
            "assets/completions/monk.fish",
            include_str!("../assets/completions/monk.fish"),
        ),
    ] {
        let mut cmd = monk::cli::cmd_factory();
        let mut buf: Vec<u8> = Vec::new();
        clap_complete::generate(shell, &mut cmd, "monk", &mut buf);
        let generated = String::from_utf8(buf).expect("completions are utf-8");
        // Compare content, not line endings: a Windows checkout with
        // core.autocrlf turns the committed LF files into CRLF, while
        // clap_complete always emits LF.
        let normalize = |s: &str| s.replace("\r\n", "\n");
        assert_eq!(
            normalize(&generated),
            normalize(bytes),
            "{path} is stale — regenerate the shipped completions with `just completions`"
        );
    }
}
