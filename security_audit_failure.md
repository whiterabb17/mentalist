un cargo audit
    Updating crates.io index
    Updating git repository `https://github.com/whiterabb17/mindpalace.git`
     Locking 571 packages to latest compatible versions
      Adding bollard v0.18.1 (available: v0.20.2)
      Adding cap-std v3.4.5 (available: v4.0.2)
      Adding colored v2.2.0 (available: v3.1.1)
      Adding criterion v0.5.1 (available: v0.8.2)
      Adding generic-array v0.14.7 (available: v0.14.9)
      Adding reqwest v0.12.28 (available: v0.13.2)
      Adding ruvector-core v0.1.31 (available: v2.1.0)
      Adding thiserror v1.0.69 (available: v2.0.18)
      Adding wasmtime v29.0.1 (available: v43.0.0)
      Adding wasmtime-wasi v29.0.1 (available: v43.0.0)
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1026 security advisories (from /home/runner/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (572 crate dependencies)
Crate:     protobuf
Version:   2.28.0
Title:     Crash due to uncontrolled recursion in protobuf crate
Date:      2024-12-12
ID:        RUSTSEC-2024-0437
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0437
Solution:  Upgrade to >=3.7.2
Dependency tree:
protobuf 2.28.0
└── prometheus 0.13.4
    └── mem-core 0.2.1
        ├── mentalist 0.3.1
        ├── mem-session 0.2.2
        │   └── brain 0.2.2
        │       ├── mentalist 0.3.1
        │       └── mem-resilience 0.1.0
        │           └── mentalist 0.3.1
        ├── mem-retriever 0.1.0
error: 6 vulnerabilities found!
warning: 6 allowed warnings found
        │   └── mentalist 0.3.1
        ├── mem-resilience 0.1.0
        ├── mem-offloader 0.1.0
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        ├── mem-micro 0.1.0
        │   └── brain 0.2.2
        ├── mem-extractor 0.2.2
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        ├── mem-dreamer 0.2.2
        │   └── brain 0.2.2
        ├── mem-compactor 0.1.0
        │   └── brain 0.2.2
        ├── mem-bridge 0.2.2
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        └── brain 0.2.2

Crate:     wasmtime
Version:   29.0.1
Title:     Panic adding excessive fields to a `wasi:http/types.fields` instance
Date:      2026-02-24
ID:        RUSTSEC-2026-0021
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0021
Severity:  6.9 (medium)
Solution:  Upgrade to >=24.0.6, <25.0.0 OR >=36.0.6, <37.0.0 OR >=40.0.4, <41.0.0 OR >=41.0.4
Dependency tree:
wasmtime 29.0.1
├── wiggle 29.0.1
│   └── wasmtime-wasi 29.0.1
│       └── mentalist 0.3.1
├── wasmtime-wasi 29.0.1
└── mentalist 0.3.1

Crate:     wasmtime
Version:   29.0.1
Title:     Guest-controlled resource exhaustion in WASI implementations
Date:      2026-02-24
ID:        RUSTSEC-2026-0020
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0020
Severity:  6.9 (medium)
Solution:  Upgrade to >=24.0.6, <25.0.0 OR >=36.0.6, <37.0.0 OR >=40.0.4, <41.0.0 OR >=41.0.4

Crate:     wasmtime
Version:   29.0.1
Title:     Host panic with `fd_renumber` WASIp1 function
Date:      2025-07-18
ID:        RUSTSEC-2025-0046
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0046
Severity:  3.3 (low)
Solution:  Upgrade to >=34.0.2 OR >=33.0.2, <34.0.0 OR >=24.0.4, <25.0.0

Crate:     wasmtime
Version:   29.0.1
Title:     Wasmtime segfault or unused out-of-sandbox load with `f64.copysign` operator on x86-64
Date:      2026-01-26
ID:        RUSTSEC-2026-0006
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0006
Severity:  4.1 (medium)
Solution:  Upgrade to >=41.0.1 OR >=40.0.3, <41.0.0 OR >=36.0.5, <37.0.0 OR <29.0.0

Crate:     wasmtime
Version:   29.0.1
Title:     Unsound API access to a WebAssembly shared linear memory
Date:      2025-11-11
ID:        RUSTSEC-2025-0118
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0118
Severity:  1.8 (low)
Solution:  Upgrade to >=38.0.4 OR >=37.0.3, <38.0.0 OR >=36.0.3, <37.0.0 OR >=24.0.5, <25.0.0

Crate:     bincode
Version:   1.3.3
Warning:   unmaintained
Title:     Bincode is unmaintained
Date:      2025-12-16
ID:        RUSTSEC-2025-0141
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0141
Dependency tree:
bincode 1.3.3
└── hnsw_rs 0.3.4
    ├── ruvector-graph 0.1.31
    │   ├── mem-retriever 0.1.0
    │   │   └── mentalist 0.3.1
    │   └── mem-core 0.2.1
    │       ├── mentalist 0.3.1
    │       ├── mem-session 0.2.2
    │       │   └── brain 0.2.2
    │       │       ├── mentalist 0.3.1
    │       │       └── mem-resilience 0.1.0
    │       │           └── mentalist 0.3.1
    │       ├── mem-retriever 0.1.0
    │       ├── mem-resilience 0.1.0
    │       ├── mem-offloader 0.1.0
    │       │   ├── mentalist 0.3.1
    │       │   └── brain 0.2.2
    │       ├── mem-micro 0.1.0
    │       │   └── brain 0.2.2
    │       ├── mem-extractor 0.2.2
    │       │   ├── mentalist 0.3.1
    │       │   └── brain 0.2.2
    │       ├── mem-dreamer 0.2.2
    │       │   └── brain 0.2.2
    │       ├── mem-compactor 0.1.0
    │       │   └── brain 0.2.2
    │       ├── mem-bridge 0.2.2
    │       │   ├── mentalist 0.3.1
    │       │   └── brain 0.2.2
    │       └── brain 0.2.2
    └── ruvector-core 0.1.31
        ├── ruvector-graph 0.1.31
        ├── mentalist 0.3.1
        └── mem-retriever 0.1.0

Crate:     bincode
Version:   2.0.1
Warning:   unmaintained
Title:     Bincode is unmaintained
Date:      2025-12-16
ID:        RUSTSEC-2025-0141
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0141
Dependency tree:
bincode 2.0.1
├── ruvector-graph 0.1.31
│   ├── mem-retriever 0.1.0
│   │   └── mentalist 0.3.1
│   └── mem-core 0.2.1
│       ├── mentalist 0.3.1
│       ├── mem-session 0.2.2
│       │   └── brain 0.2.2
│       │       ├── mentalist 0.3.1
│       │       └── mem-resilience 0.1.0
│       │           └── mentalist 0.3.1
│       ├── mem-retriever 0.1.0
│       ├── mem-resilience 0.1.0
│       ├── mem-offloader 0.1.0
│       │   ├── mentalist 0.3.1
│       │   └── brain 0.2.2
│       ├── mem-micro 0.1.0
│       │   └── brain 0.2.2
│       ├── mem-extractor 0.2.2
│       │   ├── mentalist 0.3.1
│       │   └── brain 0.2.2
│       ├── mem-dreamer 0.2.2
│       │   └── brain 0.2.2
│       ├── mem-compactor 0.1.0
│       │   └── brain 0.2.2
│       ├── mem-bridge 0.2.2
│       │   ├── mentalist 0.3.1
│       │   └── brain 0.2.2
│       └── brain 0.2.2
└── ruvector-core 0.1.31
    ├── ruvector-graph 0.1.31
    ├── mentalist 0.3.1
    └── mem-retriever 0.1.0

Crate:     fxhash
Version:   0.2.1
Warning:   unmaintained
Title:     fxhash - no longer maintained
Date:      2025-09-05
ID:        RUSTSEC-2025-0057
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0057
Dependency tree:
fxhash 0.2.1
└── fxprof-processed-profile 0.6.0
    └── wasmtime 29.0.1
        ├── wiggle 29.0.1
        │   └── wasmtime-wasi 29.0.1
        │       └── mentalist 0.3.1
        ├── wasmtime-wasi 29.0.1
        └── mentalist 0.3.1

Crate:     paste
Version:   1.0.15
Warning:   unmaintained
Title:     paste - no longer maintained
Date:      2024-10-07
ID:        RUSTSEC-2024-0436
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0436
Dependency tree:
paste 1.0.15
└── wasmtime 29.0.1
    ├── wiggle 29.0.1
    │   └── wasmtime-wasi 29.0.1
    │       └── mentalist 0.3.1
    ├── wasmtime-wasi 29.0.1
    └── mentalist 0.3.1

Crate:     rustls-pemfile
Version:   1.0.4
Warning:   unmaintained
Title:     rustls-pemfile is unmaintained
Date:      2025-11-28
ID:        RUSTSEC-2025-0134
URL:       https://rustsec.org/advisories/RUSTSEC-2025-0134
Dependency tree:
rustls-pemfile 1.0.4
└── reqwest 0.11.27
    ├── ruvector-core 0.1.31
    │   ├── ruvector-graph 0.1.31
    │   │   ├── mem-retriever 0.1.0
    │   │   │   └── mentalist 0.3.1
    │   │   └── mem-core 0.2.1
    │   │       ├── mentalist 0.3.1
    │   │       ├── mem-session 0.2.2
    │   │       │   └── brain 0.2.2
    │   │       │       ├── mentalist 0.3.1
    │   │       │       └── mem-resilience 0.1.0
    │   │       │           └── mentalist 0.3.1
    │   │       ├── mem-retriever 0.1.0
    │   │       ├── mem-resilience 0.1.0
    │   │       ├── mem-offloader 0.1.0
    │   │       │   ├── mentalist 0.3.1
    │   │       │   └── brain 0.2.2
    │   │       ├── mem-micro 0.1.0
    │   │       │   └── brain 0.2.2
    │   │       ├── mem-extractor 0.2.2
    │   │       │   ├── mentalist 0.3.1
    │   │       │   └── brain 0.2.2
    │   │       ├── mem-dreamer 0.2.2
    │   │       │   └── brain 0.2.2
    │   │       ├── mem-compactor 0.1.0
    │   │       │   └── brain 0.2.2
    │   │       ├── mem-bridge 0.2.2
    │   │       │   ├── mentalist 0.3.1
    │   │       │   └── brain 0.2.2
    │   │       └── brain 0.2.2
    │   ├── mentalist 0.3.1
    │   └── mem-retriever 0.1.0
    └── mem-core 0.2.1

Crate:     lru
Version:   0.12.5
Warning:   unsound
Title:     `IterMut` violates Stacked Borrows by invalidating internal pointer
Date:      2026-01-07
ID:        RUSTSEC-2026-0002
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0002
Dependency tree:
lru 0.12.5
└── ruvector-graph 0.1.31
    ├── mem-retriever 0.1.0
    │   └── mentalist 0.3.1
    └── mem-core 0.2.1
        ├── mentalist 0.3.1
        ├── mem-session 0.2.2
        │   └── brain 0.2.2
        │       ├── mentalist 0.3.1
        │       └── mem-resilience 0.1.0
        │           └── mentalist 0.3.1
        ├── mem-retriever 0.1.0
        ├── mem-resilience 0.1.0
        ├── mem-offloader 0.1.0
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        ├── mem-micro 0.1.0
        │   └── brain 0.2.2
        ├── mem-extractor 0.2.2
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        ├── mem-dreamer 0.2.2
        │   └── brain 0.2.2
        ├── mem-compactor 0.1.0
        │   └── brain 0.2.2
        ├── mem-bridge 0.2.2
        │   ├── mentalist 0.3.1
        │   └── brain 0.2.2
        └── brain 0.2.2

Error: Process completed with exit code 1.