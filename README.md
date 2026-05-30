# srvcs-movingaverage

Statistics microservice for srvcs.cloud: the **moving (sliding-window) average**
of a list of numbers.

This service is an orchestrator. It owns the sliding-window control flow but
delegates every arithmetic step to its dependencies:

- [`srvcs-sum`](https://github.com/srvcs/sum) — sums each window slice.
- [`srvcs-floatdivide`](https://github.com/srvcs/floatdivide) — divides each
  window sum by the window size to produce the average.

It does **not** call `srvcs-isnumber` directly; element validation propagates
from `srvcs-sum`'s `422`.

## API

### `GET /`

Service identity.

```json
{
  "service": "srvcs-movingaverage",
  "concern": "statistics: moving (sliding-window) average",
  "depends_on": ["srvcs-sum", "srvcs-floatdivide"]
}
```

### `POST /`

Request:

```json
{ "values": [1, 2, 3, 4], "window": 2 }
```

Response `200`:

```json
{ "values": [1, 2, 3, 4], "window": 2, "result": [1.5, 2.5, 3.5] }
```

#### Algorithm

`window` must be `>= 1` and `<= values.len()`, otherwise `422`. For each start
`i` in `0..=(values.len() - window)`, the window slice `values[i..i+window]` is
summed via `srvcs-sum`, then divided by `window` via `srvcs-floatdivide`; the
quotient is pushed onto `result`.

#### Status codes

- `200` — the list of windowed averages.
- `422` — `window` out of range, or a dependency rejected an input (forwarded).
- `500` — a reachable dependency returned a malformed result.
- `503` — a dependency is unavailable.

## Configuration

| Variable                 | Default                  | Description                       |
| ------------------------ | ------------------------ | --------------------------------- |
| `SRVCS_BIND_ADDR`        | `0.0.0.0:8080`           | Listen address.                   |
| `SRVCS_SUM_URL`          | `http://127.0.0.1:8088`  | Base URL of `srvcs-sum`.          |
| `SRVCS_FLOATDIVIDE_URL`  | `http://127.0.0.1:8089`  | Base URL of `srvcs-floatdivide`.  |

## Local checks

```sh
nix flake check -L
nix develop -c sh -euc 'cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test'
nix build .#default -L
```

The Linux container is exposed as `.#container`. On Apple Silicon, use
`linux/arm64` for the practical local check; CI builds the release image on
native `x86_64-linux`.

See [`srvcs/platform`](https://github.com/srvcs/platform) for the shared service
standard and CI workflow.
