# bf-rs

[![Actions Status](https://github.com/3c1u/bf-rs/workflows/Rust/badge.svg)](https://github.com/3c1u/bf-rs/actions)

**Fast** Brainfuck interpreter written in Rust.

## Prerequisites

* Rust toolchain (1.39.0 or greater)
* LLVM 8.0

## Installation

```console
cargo install --git https://github.com/3c1u/bf-rs.git
```

## Benchmarks

This table shows the time taken to run the programs on interpreters. These results were measured on a MacBook Pro (Mid 2016, i7-6700HQ).

| | bf-rs (v1.0.1) | bf-rs (opt) | [bfc](https://github.com/barracks510/bfc) | [bf02](https://github.com/3c1u/bf-interpreter) |
|:--|:-|:-|:-|:--|
|mandelbrot| 3.56 sec | 3.22 sec | 5.26 sec | 9.82 sec |
|hanoi     | 0.44 sec | 1.79 sec | 0.38 sec | 1.06 sec |
|long      | 2.07 sec | 0.87 sec | 2.51 sec | 7.30 sec |
|bench     | 0.16 sec | 0.10 sec | 0.41 sec | 0.58 sec |
|factor    | 0.83 sec | 0.67 sec | 1.19 sec | 4.00 sec |
## About example programs   

These are some programs that I have found online. I did not write any of them.

* **bench.bf** Found on [here](https://github.com/kostya/benchmarks/tree/master/brainfuck). Shows the alphabets in a reverse order.
* **mandelbrot.bf** Found on [here](https://github.com/kostya/benchmarks/tree/master/brainfuck). Prints a beautiful Mandelbrot set.
* **hanoi.bf** Found on [here](https://github.com/fabianishere/brainfuck/blob/master/examples/hanoi.bf). Solves the Tower of Hanoi problem.
* **factor.bf** Obtained from [here](https://github.com/retrage/brainfuck-rs).
* **long.bf** Obtained from [bfc](https://github.com/barracks510/bfc) repositiory.
* **oobrain.bf** Obtained from [here](https://github.com/Borisvl/brainfuck/blob/master/src/test/resources/bf/oobrain.b). Used for testing proper `u8` handling.

## License

This program is lisensed under the Apache License 2.0 or MIT License.
