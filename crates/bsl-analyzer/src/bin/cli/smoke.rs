use std::{error::Error, path::PathBuf};

pub fn run_smoke(
    source_dir: PathBuf,
    scenario_names: Vec<String>,
    budgets_path: Option<PathBuf>,
    json: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_analyzer::smoke;

    let mut scenarios = Vec::with_capacity(scenario_names.len());
    for name in &scenario_names {
        match smoke::Scenario::parse(name) {
            Ok(s) => scenarios.push(s),
            Err(e) => return Err(format!("--scenarios: {e}").into()),
        }
    }

    let budgets = smoke::Budgets::load_or_default(budgets_path.as_deref());
    let report = smoke::run(smoke::SmokeArgs { source_dir, scenarios, budgets, json });
    if report.passed() {
        Ok(())
    } else {
        Err(format!("smoke: {} budget violation(s)", report.violations.len()).into())
    }
}
