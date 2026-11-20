
# NetherNet-Mux

NetherNet-Mux is a configurable network proxy designed to manage
connections for multiple Bedrock Dedicated Server (BDS) instances. It
uses a JSON configuration file, `nethernet-mux.json`, to define each BDS
instance and the corresponding proxy listening endpoints.

This document explains how to obtain NetherNet-Mux, build it from
source, and configure `nethernet-mux.json`.

## Features

- Configure multiple BDS instances in one configuration file.
- Define a TCP listening port for each BDS instance.
- Define a UDP multiplexing listening port for each BDS instance.
- Configure the BDS service address as an IPv4 address, IPv6 address,
  or domain name.
- Configure the external SDP candidate address as an IPv4 address,
  IPv6 address, or domain name.
- Validate configuration fields and detect conflicts between proxy
  listening ports at startup.

> **Note:** The configuration options described here reflect the current
> configuration model. The availability of complete proxying and
> forwarding functionality depends on the implementation included in the
> release you are using.

## Installation

You can use a prebuilt release or compile NetherNet-Mux from source.

### Option 1: Download a release

1. Open the **Releases** page of the NetherNet-Mux project.
2. Download the archive that matches your operating system and CPU
   architecture.
3. Extract the archive.
4. Place `nethernet-mux.json` in the directory from which you intend to
   run the program.
5. Configure the file as described in [Configuration](#configuration).
6. Run the executable.

Use a release built for your platform. Do not run a binary built for a
different operating system or CPU architecture.

### Option 2: Build from source

Building from source requires a compatible Rust toolchain and Cargo.

1. Install Rust and Cargo by following the instructions on the
   [official Rust website](https://www.rust-lang.org/tools/install).

2. Clone or download the NetherNet-Mux source code.

3. Open a terminal in the project directory.

4. Build the optimized executable:

   ```bash
   cargo build --release
   ```

5. Find the executable in the `target/release/` directory.

To run the program directly through Cargo:

```bash
cargo run --release
```

The exact executable filename depends on the operating system.

## Configuration

NetherNet-Mux reads its configuration from:

```text
nethernet-mux.json
```

The file is located in the program's **current working directory**. Make
sure you create or place the file in the directory from which you launch
NetherNet-Mux.

### First launch

If `nethernet-mux.json` does not exist, NetherNet-Mux creates it with an
empty JSON array:

```json
[]
```

The program then reports that the configuration is empty and asks you to
configure the file. An empty file is also replaced with an empty JSON
array and requires configuration.

At least one BDS instance must be configured before startup can proceed.

### Configuration structure

The top-level JSON value must be an array. Each object in the array
represents one BDS instance.

| Field | Type | Required | Description |
|---|---|---|---|
| `bds_type` | String | Yes | Address type for `bds_host`: `ipv4`, `ipv6`, or `domain`. |
| `bds_host` | String | Yes | Hostname or IP address of the BDS signaling service. Its format must match `bds_type`; do not include a port. |
| `bds_tcp` | Integer | Yes | TCP port of the BDS signaling service (`1`–`65535`). |
| `listen_tcp` | Integer | Yes | TCP port where NetherNet-Mux listens for this instance's signaling requests (`1`–`65535`). |
| `listen_mux` | Integer | Yes | UDP port where NetherNet-Mux listens for this instance's game data (`1`–`65535`). |
| `public_type` | String | Yes | Address type for `public_host`: `ipv4`, `ipv6`, or `domain`. |
| `public_host` | String | Yes | Public address advertised in the SDP answer. Its format must match `public_type`; do not include a scheme or port. |
| `public_port` | Integer | Yes | Public UDP port advertised in the SDP answer (`1`–`65535`). |

All port fields are JSON integers, not strings.

### Address types

Both `bds_type` and `public_type` support the following values:

| Value | Description |
|---|---|
| `ipv4` | The corresponding host field must contain a valid IPv4 address. |
| `ipv6` | The corresponding host field must contain a valid IPv6 address. |
| `domain` | The corresponding host field must contain a valid domain name. |

`bds_type` determines how `bds_host` is validated, while
`public_type` determines how `public_host` is validated.

These two settings are independent and can have different values.

### Example: IPv4

```json
[
  {
    "bds_type": "ipv4",
    "bds_host": "192.168.1.10",
    "bds_tcp": 19132,
    "listen_tcp": 19301,
    "listen_mux": 19302,
    "public_type": "ipv4",
    "public_host": "203.0.113.10",
    "public_port": 19302
  }
]
```

### Example: IPv6

```json
[
  {
    "bds_type": "ipv6",
    "bds_host": "2001:db8::10",
    "bds_tcp": 19132,
    "listen_tcp": 19301,
    "listen_mux": 19302,
    "public_type": "ipv6",
    "public_host": "2001:db8::20",
    "public_port": 19302
  }
]
```

For IPv6 addresses, enter the address without square brackets. Do not
append the port to the address.

### Example: Domain name

```json
[
  {
    "bds_type": "domain",
    "bds_host": "bds.internal.example",
    "bds_tcp": 19132,
    "listen_tcp": 19301,
    "listen_mux": 19302,
    "public_type": "domain",
    "public_host": "edge.example.com",
    "public_port": 19302
  }
]
```

### Configure multiple BDS instances

Add one object for each BDS instance to the JSON array. Each instance
must have its own `listen_tcp` and `listen_mux` ports.

```json
[
  {
    "bds_type": "ipv4",
    "bds_host": "192.168.1.10",
    "bds_tcp": 19132,
    "listen_tcp": 19301,
    "listen_mux": 19302,
    "public_type": "domain",
    "public_host": "bds1.example.com",
    "public_port": 19302
  },
  {
    "bds_type": "ipv4",
    "bds_host": "192.168.1.11",
    "bds_tcp": 19132,
    "listen_tcp": 19303,
    "listen_mux": 19304,
    "public_type": "ipv4",
    "public_host": "203.0.113.10",
    "public_port": 19304
  }
]
```

In this example:

- The first BDS instance uses TCP port `19301` and UDP port `19302` on
  the proxy.
- The second BDS instance uses TCP port `19303` and UDP port `19304`
  on the proxy.
- The two instances have different proxy listening ports.

The example addresses use documentation-only IP ranges and domain names.
Replace them with addresses appropriate for your environment.

## Configuration validation

NetherNet-Mux checks the configuration before starting its services.

### General validation

- The configuration must contain at least one BDS instance.
- `bds_host` and `public_host` must match their declared address types.
- `bds_tcp`, `listen_tcp`, `listen_mux`, and `public_port` must not be
  `0`.
- `listen_tcp` and `listen_mux` ports must not conflict with any other
  proxy listening port in the configuration.
- TCP and UDP listening ports are also considered conflicting if their
  numeric port values are the same.

### External address validation

The value of `public_type` determines how `public_host` is validated:

| `public_type` | Expected `public_host` |
|---|---|
| `ipv4` | A valid IPv4 address, such as `203.0.113.10` |
| `ipv6` | A valid IPv6 address, such as `2001:db8::20` |
| `domain` | A domain-style hostname, such as `edge.example.com` |

The value of `bds_type` determines how `bds_host` is validated:

| `bds_type` | Expected `bds_host` |
|---|---|
| `ipv4` | A valid IPv4 address, such as `192.168.1.10` |
| `ipv6` | A valid IPv6 address, such as `2001:db8::10` |
| `domain` | A domain-style hostname, such as `bds.internal.example` |

The domain validator accepts ASCII letters, digits, and hyphens in each
label. A label cannot be empty, exceed 63 bytes, or begin or end with a
hyphen.

The full name can contain multiple labels and can have a trailing dot.
The validator does not impose a limit on the number of subdomain levels;
it applies the usual 253-byte maximum for the name without a trailing
dot.

These checks validate address syntax. They do not verify that a domain
exists in DNS, that it resolves, or that an address is reachable from
the public Internet. Private and loopback IP addresses are not rejected
solely because they are not public addresses.

### Port conflict considerations

NetherNet-Mux checks conflicts among all configured proxy listening
ports:

- `listen_tcp` ports must be unique.
- `listen_mux` ports must be unique.
- A `listen_tcp` port and a `listen_mux` port must not have the same
  numeric value, even though they use different protocols.

**Important: BDS instances deployed locally**

The configuration validator does not check BDS-side ports such as
`bds_tcp` for conflicts with proxy listening ports.

When a BDS instance is deployed locally on the same host and within the
same network namespace as NetherNet-Mux, you must ensure that its BDS
service port does not use the same port number as any configured
`listen_tcp` or `listen_mux` port across all BDS instances.

Otherwise, the BDS service and NetherNet-Mux may attempt to bind
conflicting local ports, causing a startup or connectivity failure.

This is a deployment consideration and is intentionally not enforced
by the configuration validator.

When BDS instances run in separate Docker network namespaces, their
internal service ports can generally be reused. Make sure that the
configured `bds_host` and `bds_tcp` correctly identify and reach the
intended BDS service.

### Recommended Docker setup

Run BDS in a Docker container and let NetherNet-Mux reach it through the
container's private IP address. Set `bds_host` to that address and
`bds_tcp` to the BDS signaling port.

BDS chooses its UDP port dynamically and includes the selected address
and port in the SDP answer. NetherNet-Mux reads that candidate and
forwards UDP packets to it. Configure Docker so the proxy host can reach
all TCP and UDP ports on the BDS container through its private IP; the
UDP port does not need to be known in advance.

The proxy connects to the BDS container IP. This opens all ports on the
**container** to the proxy host, not all ports on the **host** to the
Internet. Players connect to NetherNet-Mux's `listen_tcp` and
`listen_mux` ports, while BDS remains on the private Docker network.
Docker's `EXPOSE` setting alone does not make container ports reachable.

## Running and stopping

Start the executable from the directory containing `nethernet-mux.json`.

The program handles `Ctrl+C` as a shutdown request. On Unix-like
systems, it also handles `SIGTERM`.

If startup fails, the program prints an error message. On Windows, it
waits for Enter before exiting so that the error can be read when the
executable is launched by double-clicking.

## Troubleshooting

**The program says the configuration is empty**

Open `nethernet-mux.json` and add at least one BDS instance. Make sure
the top-level value is a JSON array.

**The JSON file cannot be parsed**

Check commas, quotation marks, braces, and field types. Port values must
be integers, and `bds_type` and `public_type` must each be one of
`ipv4`, `ipv6`, or `domain`.

**The host address does not match its type**

Make sure `bds_type` matches `bds_host` and `public_type` matches
`public_host`. For example, use `ipv6` for an IPv6 address and `domain`
for a hostname.

**A listening port is already in use**

Every `listen_tcp` and `listen_mux` value must be unique across all
configured instances. Choose different port numbers for the conflicting
entries.

If a BDS instance is deployed locally, also check that its BDS service
port does not conflict with any proxy listening port.

**The domain passes validation but does not connect**

Configuration validation checks the domain's syntax only. Check its DNS
records, resolution from the host running NetherNet-Mux, and network
reachability separately.

## License

NetherNet-Mux is licensed under the MIT License. See the
[LICENSE](LICENSE) file for the full license text.
