# How to play with driller

Compile driller:

```
cargo build --release
```

### Example 1 (Delayed responses)

Start a Node HTTP server from `server` directory in another terminal:

```
cd example/server
npm install
DELAY_MS=100 node server.js
```

and then run:

```
cd example
../target/release/driller run --benchmark benchmark.yml --stats
```

### Example 2 (Cookies)

Start a Node HTTP server from `server` directory in another terminal:

```
cd example/server
npm install
node server.js
```

and then run:

```
cd example
../target/release/driller run --benchmark cookies.yml --stats
```

### Example 3 (Custom headers)

Start a Node HTTP server from `server` directory in another terminal:

```
cd example/server
npm install
node server.js
```

and then run:

```
cd example
../target/release/driller run --benchmark headers.yml --stats
```

> The legacy `driller --benchmark <file>` form still works; `driller run --benchmark <file>` is the current canonical invocation.
