"""An independent reader for JETCARP pack format 1.0, written from the pack specification alone.

Levels (pack spec section 16): 0 browse (`browse`), 1 integrity (`integrity`), 2 authenticity
(`authenticity`), 3 replay (`replay`). Python 3.10+, pyarrow and the standard library; level 2 runs
`ssh-keygen -Y verify`.
"""

__version__ = "0.1.0"
FORMAT_VERSION = "1.0"
