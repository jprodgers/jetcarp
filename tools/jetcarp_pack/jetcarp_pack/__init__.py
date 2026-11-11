"""An independent reader for JETCARP pack format 1.0, written from the spec and the book alone.

Levels (pack-format.md, "Conformance levels"): 0 browse (`browse`), 1 integrity (`integrity`), 2 authenticity
(`authenticity`), 3 replay (`replay`). Python 3.10+, pyarrow and the standard library; level 2 runs
`ssh-keygen -Y verify`.
"""

__version__ = "0.1.0"
FORMAT_VERSION = "1.0"
