# overlink

Overlink is a simple library that works to override linked C functions, while
also keeping it conventient to delegate to the function being overridden.

## Usage

To override an existing C function, just use the `#[overlink]` attribute:

```rust
#[overlink]
unsafe extern fn time(arg: *mut time_t) -> time_t {
  let result = super(arg);
  println!("time -> {result:?}");
  result
}
```

### `name`

`#[overlink(name = "symbol_name")]` can be used to specify a linked/exported name
that is different from the function name:

```rust
#[overlink(name = "time")]
unsafe extern fn custom_time(arg: *mut time_t) -> time_t {
  // ...
}
```

By default, the symbol name is the same as the function name (effectively working
like `#[no_mangle]`).

### `allow_reentry`

By default, if the function is called within itself, it will automatically call
the overridden function itself. This is to protect from cases where a overridden
function is indirectly called from within the `#[overlink]` function, causing an
unintented recursive loop.

`#[overlink(allow_reentry)]` can be used to disable this behavior.

> [!CAUTION]
> When the default behavior is disabled, you must take care to ensure that the
> function does not create an uncontrolled recursive loop, as this may lead to
> stack overflow.
