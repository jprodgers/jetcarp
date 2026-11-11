"""The limits of pack-format.md ("Limits"), plus the reader's own budgets for replay."""

from dataclasses import dataclass

KiB, MiB, GiB = 2 ** 10, 2 ** 20, 2 ** 30


@dataclass
class Limits:
    object_bytes: int = 4 * GiB  # any object
    document_bytes: int = 16 * MiB  # manifest, index, ref, key statement, receipt, signature
    parts_per_index: int = 10_000  # also applied to an index's children
    index_depth: int = 8  # the top index is depth 1
    json_depth: int = 32
    string_cell: int = 1 * MiB
    phase_cells: int = 1_000_000
    period: int = 2 ** 32
    decompression_ratio: int = 64  # Parquet's uncompressed size over the part's stored size
    # Not in the spec: what decoded Arrow data may add on top of ratio x stored size. Dictionary
    # and run-length encoding make decoded columns far bigger than Parquet's uncompressed pages.
    decoded_slack: int = 64 * MiB
    batch_rows: int = 64  # rows decoded at a time, so one batch stays small
    # Replay (level 3) budgets per record.
    replay_generations: int = 100_000
    replay_cells: int = 1_000_000
    replay_seconds: float = 20.0
    subprocess_seconds: float = 30.0


DEFAULT = Limits()
