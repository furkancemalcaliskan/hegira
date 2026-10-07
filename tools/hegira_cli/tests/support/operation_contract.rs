use serde_json::{Value, json};

// Expected data is independently reviewed, not serialized from the production planner.
// Substitute only fixture identity/provider values; never normalize observed output.
pub fn plan(application: &str, composition: &str, database: &str, operation: &str) -> Value {
    assert!(matches!(database, "sqlite" | "postgres"));
    let profile = if database == "sqlite" {
        "sqlite"
    } else {
        "development"
    };
    let fixture: Value = serde_json::from_str(
        &include_str!("../snapshots/operations/plans.json")
            .replace("__APPLICATION__", application)
            .replace("@@DATABASE@@", database)
            .replace("__PROFILE__", profile),
    )
    .unwrap();
    let mut plan = fixture["common"].as_object().unwrap().clone();
    for section in [
        &fixture["compositions"][composition],
        &fixture["operations"][operation],
    ] {
        for (key, value) in section.as_object().unwrap() {
            assert!(plan.insert(key.clone(), value.clone()).is_none());
        }
    }
    let prerequisites = plan
        .get_mut("prerequisites")
        .unwrap()
        .as_array_mut()
        .unwrap();
    if operation == "dev" || operation == "build" {
        prerequisites.extend(
            fixture["frontend_prerequisites"]
                .as_array()
                .unwrap()
                .clone(),
        );
        let key = if operation == "dev" {
            "development_prerequisites"
        } else {
            "release_prerequisites"
        };
        prerequisites.extend(fixture[key].as_array().unwrap().clone());
        if operation == "dev" && database == "postgres" {
            prerequisites.extend(
                fixture["postgres_development_prerequisites"]
                    .as_array()
                    .unwrap()
                    .clone(),
            );
        }
    }
    Value::Object(plan)
}

pub fn preview(plan: Value) -> Value {
    json!({ "output_schema": 1, "mode": "preview", "plan": plan,
        "execution": null, "diagnostics": [] })
}

pub fn human_preview(application: &str, database: &str, operation: &str) -> String {
    assert!(matches!(database, "sqlite" | "postgres"));
    let snapshot = match operation {
        "check" => include_str!("../snapshots/operations/check-preview.txt"),
        "test" => include_str!("../snapshots/operations/test-preview.txt"),
        "dev" => include_str!("../snapshots/operations/dev-preview.txt"),
        "build" => include_str!("../snapshots/operations/build-preview.txt"),
        _ => panic!("unknown fixture operation"),
    };
    let sqlite = database == "sqlite";
    let mut expected = snapshot
        .replace("__APPLICATION__", application)
        .replace("@@DATABASE@@", database)
        .replace(
            "@@DATABASE_DEBUG@@",
            if sqlite { "Sqlite" } else { "Postgres" },
        )
        .replace("__PROFILE__", if sqlite { "sqlite" } else { "development" })
        .replace(
            "@@PROFILE_DEBUG@@",
            if sqlite { "Sqlite" } else { "Development" },
        );
    if operation == "dev" && !sqlite {
        expected.push_str("Required (not probed): PostgresService\n");
    }
    expected
}

pub fn outcome(operation: &str, case: &str, plan: &Value) -> Value {
    let intent = match operation {
        "check" => "Check",
        "test" => "Test",
        "dev" => "Develop",
        "build" => "ReleaseBuild",
        _ => panic!("unknown fixture operation"),
    };
    let total = if matches!(operation, "check" | "test") {
        2
    } else {
        1
    };
    let cases: Value = serde_json::from_str(
        &include_str!("../snapshots/operations/outcomes.json")
            .replace(
                "__OPERATION__",
                plan["intent"]["operation"].as_str().unwrap(),
            )
            .replace("__INTENT__", intent)
            .replace("__COMPLETED__", &total.to_string())
            .replace("__TOTAL__", &total.to_string()),
    )
    .unwrap();
    let mut expected = cases[case].clone();
    assert!(expected.is_object());
    if case == "success" {
        expected["execution"]["completed_steps"] = json!(total);
        if operation == "build" {
            expected["execution"]["artifacts"] = plan["artifacts"].clone();
            let stdout = expected["stdout"].as_str().unwrap().to_owned()
                + include_str!("../snapshots/operations/release-artifacts.txt");
            expected["stdout"] = json!(stdout);
        }
    }
    expected
}

pub fn execution(plan: Value, expected: &Value) -> Value {
    json!({ "output_schema": 1, "mode": "execute", "plan": plan,
        "execution": expected["execution"], "diagnostics": expected["diagnostics"] })
}
