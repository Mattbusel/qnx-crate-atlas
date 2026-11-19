# `atlas claims` examples

Two crates that both claim `no_std`. One is telling the truth.

| Crate | Claim | `atlas claims` |
|---|---|---|
| [`honest-no-std`](honest-no-std) | `categories = ["no-std"]`, `#![no_std]` | passes on `thumbv7em-none-eabihf` |
| [`broken-no-std`](broken-no-std) | `categories = ["no-std", "embedded"]`, `keywords = ["no_std"]` | fails: `memchr` keeps its default `std` feature |

```console
atlas claims examples/claims/honest-no-std --qnx off    # exit 0
atlas claims examples/claims/broken-no-std --qnx off    # exit 1, error[E0463] in memchr, plus who enabled std
```

CI runs both on every push (`.github/workflows/ci.yml`).
