mod common;

#[test]
fn kept_net_cases_pass() {
    let Some(c) = common::content() else { return };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/net/cases");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    paths.sort();
    for p in paths {
        let case = rl_cli::netsim::NetCase::load(&p).unwrap();
        let r = rl_cli::netsim::run(&c, &case.spec).unwrap();
        assert!(r.verdict.is_ok(), "{}: {:?}", p.display(), r.verdict);
    }
}
