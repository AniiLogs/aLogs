# aLogs (Rust rewrite)

Local work in progress: a Rust port of aLogs 0.1.37 B16 and the Sanctum Helper it bundles. The plan, the reference
refs and how parity is checked are in [docs/rewrite-plan.md](docs/rewrite-plan.md).

```
git clone https://github.com/AniiLogs/aLogs reference/aLogs && git -C reference/aLogs checkout e340b39
git clone https://github.com/AniiLogs/SanctumHelper reference/SanctumHelper && git -C reference/SanctumHelper checkout 8cd1c6a
cargo test
python -I tools/gen_hpcalc_goldens.py     # rewrites the hpcalc goldens from the reference
```
