# srvcs-movingaverage

## Name

| Field | Value |
| --- | --- |
| Service | `srvcs-movingaverage` |
| Slug | `movingaverage` |
| Repository | `srvcs/movingaverage` |
| Package | `srvcs-movingaverage` |
| Kind | `orchestrator` |

## Function

statistics: moving (sliding-window) average

## Dependencies

| Dependency | Repository |
| --- | --- |
| `srvcs-sum` | [srvcs/sum](https://github.com/srvcs/sum) |
| `srvcs-floatdivide` | [srvcs/floatdivide](https://github.com/srvcs/floatdivide) |

## API

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Service identity |
| `POST` | `/` | Evaluate the service function |
| `GET` | `/healthz` | Liveness probe |
| `GET` | `/readyz` | Readiness probe |
| `GET` | `/metrics` | Prometheus metrics |
| `GET` | `/openapi.json` | OpenAPI document |

## Inputs

| Name | Type | Required |
| --- | --- | --- |
| `values` | `json[]` | yes |
| `window` | `integer` | yes |

## Outputs

| Name | Type |
| --- | --- |
| `values` | `json[]` |
| `window` | `integer` |
| `result` | `number[]` |

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `SRVCS_BIND_ADDR` | `0.0.0.0:8080` | Bind address |
| `SRVCS_ENV` | `development` | Environment label for logs |
| `RUST_LOG` | `info,tower_http=info` | Tracing filter |
| `SRVCS_FLOATDIVIDE_URL` | `http://127.0.0.1:8089` | Base URL for srvcs-floatdivide |
| `SRVCS_SUM_URL` | `http://127.0.0.1:8088` | Base URL for srvcs-sum |

## Error Behavior

- `422` means the request could not be evaluated for the documented input shape.
- `503` means a required dependency was unavailable or returned an unexpected response.
- Dependency validation errors are forwarded when this service delegates validation.

## Local Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

See the [srvcs service standard](https://github.com/srvcs/platform/blob/main/STANDARD.md) for the full operational contract.

## Metadata

Machine-readable service metadata lives in `srvcs.yaml`. Keep it aligned with this README when the service contract changes.
