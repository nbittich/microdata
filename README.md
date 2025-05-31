# Microdata

microdata parser.

## Usage:

```rust
use microdata::parse_html;

  let html = r#"
        <div itemscope>
            <p>My name is <span itemprop="name">Elizabeth</span>.</p>
        </div>
        <div itemscope>
            <p>My name is <span itemprop="name">Daniel</span>.</p>
        </div>
  "#;
  let res = parse_html("", html).unwrap();
```
