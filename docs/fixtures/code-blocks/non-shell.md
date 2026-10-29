# Non-shell blocks: Copy only

None of these should show **▶ Run**.
daddada
```sql
-- Postgres
SELECT now() AS server_time, version();

CREATE TEMP TABLE fixture_users (
  id    serial PRIMARY KEY,
  name  text NOT NULL,
  email text UNIQUE
);
INSERT INTO fixture_users (name, email)
VALUES ('Ada', 'ada@example.com'), ('Linus', 'linus@example.com');
SELECT * FROM fixture_users ORDER BY id;
```

```psql
\dt
\x on
SELECT 1;
```

```rust
fn main() {
    println!("Hello from Rust");
}
```

```typescript
const greet = (name: string): string => `Hello, ${name}`;
console.log(greet("Sawit"));
```

```python
def greet(name: str) -> str:
    return f"Hello, {name}"
```

```json
{ "name": "sawit", "version": "0.0.3", "features": ["run", "copy"] }
```

```yaml
services:
  db:
    image: postgres:17
    ports: ["5432:5432"]
```

```toml
[package]
name = "sawit"
```

```dockerfile
FROM postgres:17
ENV POSTGRES_PASSWORD=postgres
```

```diff
- echo "old"
+ echo "new"
```

```makefile
run:
	cargo run -p sawit -- --foreground
```

Inline code like `echo "inline"` has no buttons at all.
