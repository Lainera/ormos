# Reverse proxy for SNI routing.
                                                                                                
## Why?

In an ideal world anyone would be able to host their software. Unfortunately, with IPv4 address
exhaustion and limited adoption of IPv6 infrastructure this is not a reality we live in.
                                                                                                
One way to address the issue would be to host software in public cloud. This, however, means
paying for the infrastructure, and, exposing encryption keys (assuming there is TLS involved) to the public cloud.

If you would like to keep your keys to yourself and minimize the infrastructure bills, another
way to address the issue would be to deploy TCP load balancer in public cloud and use SNI to
chose downstream service.
                                                                                                
## How?

[![](https://mermaid.ink/img/pako:eNp1UU9LwzAU_yqPnCbMOT14CDKQdspARjG7GQ9p8maDa1KTtLOMfXdf7RR2WMgheb9_7_EOTHuDjLOIXy06jblVH0HV0jUqJKtto1yCDFSEbGfRpXOgGACxXkER_Hd_juUDlq8FCAwdhnNQDKDxexdTQFXPInGsxpn2FC2d8wnBkwryKQgO-T8TTkywER7KcLMAqupKlTuEzipYFd09TJxKtiOHADXG6kq67HqxKDhsXsTkebm5kEy8YuS9ojLDXGMh5_BIh5K0Dwa2ZHup9XzUv93N57fckAvnJeL2fTSiSTYVNU53MOl9K50YFUunQ98kNJQSG-8ijpKMw5MPe0W5ZZ8wQvKgfzfBpqzGUCtraH0H6QAkSxXWKBmnp1HhUzLpjsRrG6MSLo1NPjCeQotTptrkRe_033_knNY_Fo8_yDW74Q)](https://mermaid-js.github.io/mermaid-live-editor/edit#pako:eNp1UU9LwzAU_yqPnCbMOT14CDKQdspARjG7GQ9p8maDa1KTtLOMfXdf7RR2WMgheb9_7_EOTHuDjLOIXy06jblVH0HV0jUqJKtto1yCDFSEbGfRpXOgGACxXkER_Hd_juUDlq8FCAwdhnNQDKDxexdTQFXPInGsxpn2FC2d8wnBkwryKQgO-T8TTkywER7KcLMAqupKlTuEzipYFd09TJxKtiOHADXG6kq67HqxKDhsXsTkebm5kEy8YuS9ojLDXGMh5_BIh5K0Dwa2ZHup9XzUv93N57fckAvnJeL2fTSiSTYVNU53MOl9K50YFUunQ98kNJQSG-8ijpKMw5MPe0W5ZZ8wQvKgfzfBpqzGUCtraH0H6QAkSxXWKBmnp1HhUzLpjsRrG6MSLo1NPjCeQotTptrkRe_033_knNY_Fo8_yDW74Q)

<details>
<summary> Mermaid code under spoiler </summary>

```mermaid
sequenceDiagram
participant C as Client
participant P as SNI Proxy
participant D as DNS Server
participant S as downstream.service.com

note over D, S: Downstream service is <br/> reachable via IPv6 (native or mesh)
C->>P: TLS(GET downstream.service.com)
P->>P: Read SNI
P->>D: AAAA record for downstream.service.com
D->>P: [2001:dead::beef]
P->>S: This is for you
S->>P: Encrypted response
P->>C: Forward bytes to Client
```
</details>


## Installation

- [Get rust](https://rustup.rs/)
- Build with `cargo build --bin rpx --features instrument --release`. Alternatively you may cross-compile with `docker buildx` - see `Makefile` for inspiration.  
- Take a look at [`sample_config.yml`](./sample_config.yml), punch in values relevant for your use-case 

## More docs

- Run `cargo doc --open` 
