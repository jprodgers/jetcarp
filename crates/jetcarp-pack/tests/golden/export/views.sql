-- DuckDB views over JETCARP pack version sha256:8dbe85670c96804f87db94880c6b1d702d6eae7964fc6fdae11d86fcd3123ed6 (feed golden-a).
-- Not part of the format: run `duckdb -init views.sql` in this folder, or replace
-- the paths with this folder's URL to query a pack in place over HTTPS.
CREATE OR REPLACE VIEW annotations AS SELECT * FROM read_parquet(['tables/annotations/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW patterns AS SELECT * FROM read_parquet(['tables/patterns/class_group=oscillator/tiling_group=square/part-000001.parquet', 'tables/patterns/class_group=spaceship/tiling_group=square/part-000001.parquet', 'tables/patterns/class_group=still_life/tiling_group=square/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW provenance AS SELECT * FROM read_parquet(['tables/provenance/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW quickstart AS SELECT * FROM read_parquet(['tables/quickstart/tiling_group=square/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW raters AS SELECT * FROM read_parquet(['tables/raters/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW ratings AS SELECT * FROM read_parquet(['tables/ratings/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW rules AS SELECT * FROM read_parquet(['tables/rules/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW sightings AS SELECT * FROM read_parquet(['tables/sightings/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW tilings AS SELECT * FROM read_parquet(['tables/tilings/part-000001.parquet'], union_by_name = true);
CREATE OR REPLACE VIEW verifications AS SELECT * FROM read_parquet(['tables/verifications/part-000001.parquet'], union_by_name = true);
