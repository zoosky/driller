# Driller

[![](https://img.shields.io/crates/v/driller.svg?ts=2)](https://crates.io/crates/driller)

Driller is a HTTP load testing application written in Rust. Friendly fork of
[fcsonline/drill](https://github.com/fcsonline/drill) — see [FORK.md](./FORK.md)
for the rationale and the relationship to upstream.

The main goal for this project is to build a really lightweight tool as
alternative to other that require JVM and other stuff.

You can write benchmark files, in YAML format, describing all the stuff you
want to test.

It was inspired by [Ansible](http://docs.ansible.com/ansible/playbooks_intro.html)
syntax because it is really easy to use and extend.

Here is an example for **benchmark.yml**:

```yaml
---

concurrency: 4
base: 'http://localhost:9000'
iterations: 5
rampup: 2

plan:
  - name: Include comments
    include: comments.yml

  - name: Fetch users
    request:
      url: /api/users.json

  - name: Fetch organizations
    request:
      url: /api/organizations

  - name: Fetch account
    request:
      url: /api/account
    assign: foo

  - name: Fetch manager user
    request:
      url: /api/users/{{ foo.body.manager_id }}

  - name: Assert request response code
    assert:
      key: foo.status
      value: 200

  - name: Assign values
    assign:
      key: bar
      value: "2"

  - name: Assert values
    assert:
      key: bar
      value: "2"

  - name: Fetch user from assign
    request:
      url: /api/users/{{ bar }}

  - name: Fetch some users
    request:
      url: /api/users/{{ item }}
    with_items:
      - 70
      - 73
      - 75

  - name: Tagged user request
    request:
      url: /api/users/70
    tags:
      - tag_user

  - name: Fetch some users by hash
    request:
      url: /api/users/{{ item.id }}
    with_items:
      - { id: 70 }
      - { id: 73 }
      - { id: 75 }

  - name: Fetch some users by range, index {{ index }}
    request:
      url: /api/users/{{ item }}
    with_items_range:
      start: 70
      step: 5
      stop: 75

  - name: Fetch some users from CSV, index {{ index }}
    request:
      url: /api/users/contacts/{{ item.id }}
    with_items_from_csv: ./fixtures/users.csv
    shuffle: true

  - name: POST some crafted JSONs stored in CSV, index {{ index }}
    request:
      url: /api/transactions
      method: POST
      body: '{{ item.txn }}'
      headers:
        Content-Type: 'application/json'
    with_items_from_csv:
      file_name: ./fixtures/transactions.csv
      quote_char: "\\'"

  - name: Fetch no relative url
    request:
      url: http://localhost:9000/api/users.json

  - name: Interpolate environment variables
    request:
      url: http://localhost:9000/api/{{ EDITOR }}

  - name: Support for POST method
    request:
      url: /api/users
      method: POST
      body: foo=bar&arg={{ bar }}

  - name: Support for hex-encoded request body
    request:
      url: /api/blob
      method: POST
      body:
        hex: 65 78 61 6D 70 6C 65

  - name: Support request body loaded from a file
    request:
      url: /api/blob
      method: POST
      body:
        file: ./image.png

  - name: Login user
    request:
      url: /login?user=example&password=3x4mpl3

  - name: Fetch counter
    request:
      url: /counter
    assign: memory

  - name: Fetch counter
    request:
      url: /counter
    assign: memory

  - name: Fetch endpoint
    request:
      url: /?counter={{ memory.body.counter }}

  - name: Reset counter
    request:
      method: DELETE
      url: /

  - name: Exec external commands
    exec:
      command: "echo '{{ foo.body }}' | jq .phones[0] | tr -d '\"'"
    assign: baz

  - name: Custom headers
    request:
      url: /admin
      headers:
        Authorization: Basic aHR0cHdhdGNoOmY=
        X-Foo: Bar
        X-Bar: Bar {{ memory.headers.token }}

  - name: One request with a random item
    request:
      url: /api/users/{{ item }}
    with_items:
      - 70
      - 73
      - 75
    shuffle: true
    pick: 1

  - name: Three requests with random items from a range
    request:
      url: /api/users/{{ item }}
    with_items_range:
      start: 1
      stop: 1000
    shuffle: true
    pick: 3
```

As you can see, you can play with interpolations in different ways. This
will let you specify a benchmark with different requests and
dependencies between them.

If you want to know more about the benchmark file syntax, [read this](./SYNTAX.md)

## Install

Right now, the easiest way to get `driller` is to go to the
[latest release](https://github.com/zoosky/driller/releases/latest)
page and download the binary file for your platform.


Another way to install `driller`, if you have [Rust](https://rustup.rs/) available in
your system, is with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html):

```
cargo install driller
driller --benchmark benchmark.yml --stats
```

or download the source code and compile it:

```
git clone git@github.com:zoosky/driller.git && cd driller
cargo build --release
./target/release/driller --benchmark benchmark.yml --stats
```

### Dependencies

OpenSSL is needed in order to compile Driller, whether it is through `cargo install`
or when compiling from source with `cargo build`.

Depending on your platform, the name of the dependencies may differ.

#### Linux

Install `libssl-dev` and `pkg-config` packages with your favorite package manager
(if `libssl-dev` is not found, try other names like `openssl` or `openssl-devel`).

#### macOS

First, install the [Homebrew](https://brew.sh/) package manager.

And then install `openssl` with Homebrew.

#### Windows

First, install [vcpkg](https://vcpkg.io/en/getting-started.html).

And then run `vcpkg install openssl:x64-windows-static-md`.

## Demo

[![demo](https://asciinema.org/a/164615.png)](https://asciinema.org/a/164615?autoplay=1)

## Features

This is the list of all features supported by the current version of `driller`:

- **Concurrency:** run your benchmarks choosing the number of concurrent iterations.
- **Multi iterations:** specify the number of iterations you want to run the benchmark.
- **Ramp-up:** specify the amount of time, in seconds, that it will take `driller` to start all iterations.
- **Delay:** introduce controlled delay between requests. Example: [delay.yml](./example/delay.yml)
- **Dynamic urls:** execute requests with dynamic interpolations in the url, like `/api/users/{{ item }}`
- **Dynamic headers:** execute requests with dynamic headers. Example: [headers.yml](./example/headers.yml)
- **Interpolate environment variables:** set environment variables, like `/api/users/{{ EDITOR }}`
- **Executions:** execute remote commands with test plan data.
- **Assertions:** assert values during the test plan. Example: [iterations.yml](./example/iterations.yml)
- **Request dependencies:** create dependencies between requests with `assign` and url interpolations.
- **Split files:** organize your benchmarks in multiple files and include them.
- **CSV support:** read CSV files and build N requests fill dynamic interpolations with CSV data.
- **HTTP methods:** build request with different http methods like GET, POST, PUT, PATCH, HEAD or DELETE.
- **Cookie support:** create benchmarks with sessions because cookies are propagates between requests.
- **Stats:** get nice statistics about all the requests. Example: [cookies.yml](./example/cookies.yml)
- **Thresholds:** compare the current benchmark performance against a stored one session and fail if a threshold is exceeded.
- **Tags:** specify test plan items by tags.

## Test it

Go to the `example` directory and you'll find a [README](./example) how
to test it in a safe environment.

**Disclaimer**: We really recommend not to run intensive benchmarks against
production environments.

## Command line interface

Full list of cli options, which is available under `driller --help`

```
driller 1.0.0-alpha.1
HTTP load testing application written in Rust inspired by Ansible syntax

USAGE:
    driller [FLAGS] [OPTIONS] --benchmark <benchmark>

FLAGS:
    -h, --help                      Prints help information
        --list-tags                 List all benchmark tags
        --list-tasks                List benchmark tasks (executes --tags/--skip-tags filter)
    -n, --nanosec                   Shows statistics in nanoseconds
        --no-check-certificate      Disables SSL certification check. (Not recommended)
    -q, --quiet                     Disables output
        --relaxed-interpolations    Do not panic if an interpolation is not present. (Not recommended)
    -s, --stats                     Shows request statistics
    -V, --version                   Prints version information
    -v, --verbose                   Toggle verbose output

OPTIONS:
    -b, --benchmark <benchmark>    Sets the benchmark file
    -c, --compare <compare>        Sets a compare file
    -r, --report <report>          Sets a report file
        --skip-tags <skip-tags>    Tags to exclude
        --tags <tags>              Tags to include
    -t, --threshold <threshold>    Sets a threshold value in ms amongst the compared file
    -o, --timeout <timeout>        Set timeout in seconds for all requests
```

## Roadmap

- Complete and improve the interpolation engine
- Add writing to a file support

## Credits

Driller is a fork of [fcsonline/drill](https://github.com/fcsonline/drill) by
Ferran Basora and contributors, who built the original tool as a side project
to learn Rust. If you appreciate the work that went into the upstream:

[!["Buy Ferran a Coffee"](https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png)](https://www.buymeacoffee.com/fcsonline)

## Contribute

Pull requests are welcome. See [FORK.md](./FORK.md) for the relationship to
upstream and our goals for this fork.

