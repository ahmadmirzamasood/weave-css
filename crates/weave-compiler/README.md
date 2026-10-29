# weave-compiler

The compiler library behind the [Weave](https://github.com/ahmadmirzamasood/weave-css) stylesheet language. It compiles `.weave` source to standard CSS and optionally an ECMA-426 source map. It is pure Rust, has no dependencies, forbids `unsafe` code, and performs no filesystem, network, environment or clock access: module files are read through a `ModuleResolver` you provide.

This is a **release candidate (0.1.0-rc.3)**. The API may change before 1.0 (see the [compatibility policy](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/COMPATIBILITY-POLICY.md)). The minimum supported Rust version is 1.85.

```rust
use weave_compiler::{compile, CompileOptions, MemoryResolver, ModuleId};

let entry = ModuleId::new("main.weave").unwrap();
let mut files = MemoryResolver::new();
files.insert(entry.clone(), "bind brand = #0b5fff;
rule .button { color: #{brand}; }
");

let compilation = compile(&entry, &mut files, &CompileOptions::default());
match &compilation.outcome {
    Ok(output) => assert_eq!(output.css, ".button {
  color: #0b5fff;
}
"),
    Err(diagnostics) => {
        for diagnostic in diagnostics {
            eprintln!("{}", diagnostic.render(&compilation.sources));
        }
    }
}
```

Documentation: [library API guide](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/LIBRARY.md), [language reference](https://github.com/ahmadmirzamasood/weave-css/blob/main/docs/LANGUAGE.md), [changelog](https://github.com/ahmadmirzamasood/weave-css/blob/main/CHANGELOG.md). Licensed under the MIT license.
