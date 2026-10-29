-- Sawit SQL editor fixture. Safe to run: everything is in a TEMP table,
-- which disappears when the connection closes.

SELECT now() AS server_time, current_user, current_database(), version();

CREATE TEMP TABLE fixture_orders (
  id          serial PRIMARY KEY,
  customer    text        NOT NULL,
  total       numeric(10, 2) NOT NULL CHECK (total >= 0),
  status      text        NOT NULL DEFAULT 'pending',
  tags        text[]      NOT NULL DEFAULT '{}',
  meta        jsonb       NOT NULL DEFAULT '{}',
  created_at  timestamptz NOT NULL DEFAULT now()
);

INSERT INTO fixture_orders (customer, total, status, tags, meta) VALUES
  ('Ada',    120.50, 'paid',    '{vip}',          '{"channel": "web"}'),
  ('Linus',   15.00, 'pending', '{}',             '{"channel": "app"}'),
  ('Grace',  999.99, 'paid',    '{vip,wholesale}','{"channel": "web", "coupon": "NAVY"}'),
  ('Alan',     0.00, 'refunded','{}',             '{}'),
  ('Ünïcødé', 42.00, 'paid',    '{"emoji 🌴"}',    '{"note": "unicode row"}'),
  ('Null meta', 7.25, 'pending', '{}',            'null');

-- Grid: many types, NULL-ish values, unicode
SELECT * FROM fixture_orders ORDER BY id;

-- Aggregates
SELECT status, count(*) AS orders, sum(total) AS revenue, round(avg(total), 2) AS avg_total
FROM fixture_orders
GROUP BY status
ORDER BY revenue DESC;

-- jsonb and arrays
SELECT customer, meta ->> 'channel' AS channel, 'vip' = ANY (tags) AS is_vip
FROM fixture_orders
WHERE meta ? 'channel';

-- Window function
SELECT customer, total,
       rank() OVER (ORDER BY total DESC) AS rank,
       sum(total) OVER () AS grand_total
FROM fixture_orders;

-- Many rows (scrolling / virtualized grid)
SELECT g AS n, md5(g::text) AS hash, now() - (g || ' minutes')::interval AS at
FROM generate_series(1, 5000) AS g;

-- Wide result (horizontal scroll)
SELECT 1 AS c01, 2 AS c02, 3 AS c03, 4 AS c04, 5 AS c05, 6 AS c06, 7 AS c07, 8 AS c08,
       9 AS c09, 10 AS c10, 11 AS c11, 12 AS c12, 13 AS c13, 14 AS c14, 15 AS c15, 16 AS c16,
       'a very long text value that should be truncated or wrapped in the grid cell' AS long_text;

-- Update / delete report affected rows
UPDATE fixture_orders SET status = 'shipped' WHERE status = 'paid';
DELETE FROM fixture_orders WHERE total = 0;

-- Error handling: these should show a clear error, not crash
SELECT * FROM table_that_does_not_exist;
SELECT 1 / 0;
INSERT INTO fixture_orders (customer, total) VALUES ('Bad', -1);

-- Slow query (cancel handling)
SELECT pg_sleep(5), 'done' AS result;
