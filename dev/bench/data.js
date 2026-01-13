window.BENCHMARK_DATA = {
  "lastUpdate": 1768279278331,
  "repoUrl": "https://github.com/jakekaplan/loq",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "email": "40362401+jakekaplan@users.noreply.github.com",
            "name": "Jake Kaplan",
            "username": "jakekaplan"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "22e2506ec68446192b648cf16bde1531c7b09afc",
          "message": "Merge pull request #11 from jakekaplan/ci-benchmarks\n\nAdd CI benchmarks",
          "timestamp": "2026-01-12T23:40:00-05:00",
          "tree_id": "f9ef2deb88b4340df14b09fc6586e3e6cab5ec6a",
          "url": "https://github.com/jakekaplan/loq/commit/22e2506ec68446192b648cf16bde1531c7b09afc"
        },
        "date": 1768279277585,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "cpython",
            "value": 0.05354809244000001,
            "range": "± 0.0049",
            "unit": "seconds"
          },
          {
            "name": "airflow",
            "value": 0.14425320634000002,
            "range": "± 0.0005",
            "unit": "seconds"
          },
          {
            "name": "prefect",
            "value": 0.059130017940000015,
            "range": "± 0.0008",
            "unit": "seconds"
          },
          {
            "name": "ruff",
            "value": 0.12522010454000002,
            "range": "± 0.0008",
            "unit": "seconds"
          }
        ]
      }
    ]
  }
}