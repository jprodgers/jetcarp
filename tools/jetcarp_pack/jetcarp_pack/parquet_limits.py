"""The Parquet limits of pack-format.md ("Limits"), checked before any page is decoded:
- the footer at most 16 MiB, nested at most 32 deep in Thrift, at most 1,024 columns;
- the sum of the pages' uncompressed sizes, as their headers say, at most 64 times the part's size;
- the data pages' values at most 64 per byte of the part, plus 2^20.

pyarrow does not show page headers, so they are read here with a small Thrift compact-protocol
reader that refuses deep nesting and containers larger than the bytes left.
"""

import struct

FOOTER_MAX = 16 << 20
THRIFT_DEPTH = 32
MAX_COLUMNS = 1024
_HEADER_WINDOW = 2 << 20
DATA_PAGE, INDEX_PAGE, DICTIONARY_PAGE, DATA_PAGE_V2 = 0, 1, 2, 3


class ParquetLimitError(ValueError):
    pass


class _Thrift:
    def __init__(self, buf, max_depth=THRIFT_DEPTH):
        self.buf, self.pos, self.max_depth = buf, 0, max_depth

    def byte(self):
        if self.pos >= len(self.buf):
            raise ParquetLimitError("Thrift data is cut short")
        b = self.buf[self.pos]
        self.pos += 1
        return b

    def varint(self):
        shift = result = 0
        while True:
            b = self.byte()
            result |= (b & 0x7F) << shift
            if not b & 0x80:
                return result
            shift += 7
            if shift > 70:
                raise ParquetLimitError("a Thrift varint is too long")

    def zigzag(self):
        n = self.varint()
        return (n >> 1) ^ -(n & 1)

    def value(self, ttype, depth, in_container=False):
        if ttype in (1, 2):
            return (self.byte() == 1) if in_container else (ttype == 1)
        if ttype == 3:
            return self.byte()
        if ttype in (4, 5, 6):
            return self.zigzag()
        if ttype == 7:
            self.pos += 8
            if self.pos > len(self.buf):
                raise ParquetLimitError("Thrift data is cut short")
            return None
        if ttype == 8:
            n = self.varint()
            if n > len(self.buf) - self.pos:
                raise ParquetLimitError("a Thrift string runs past its data")
            s = self.buf[self.pos:self.pos + n]
            self.pos += n
            return s
        if ttype in (9, 10):
            h = self.byte()
            size, et = h >> 4, h & 0x0F
            if size == 15:
                size = self.varint()
            if size > len(self.buf) - self.pos:
                raise ParquetLimitError("a Thrift list is longer than its data")
            return [self.value(et, depth + 1, True) for _ in range(size)]
        if ttype == 11:
            size = self.varint()
            if size and size > len(self.buf) - self.pos:
                raise ParquetLimitError("a Thrift map is longer than its data")
            out = []
            if size:
                kv = self.byte()
                for _ in range(size):
                    out.append((self.value(kv >> 4, depth + 1, True), self.value(kv & 0x0F, depth + 1, True)))
            return out
        if ttype == 12:
            return self.struct(depth + 1)
        raise ParquetLimitError(f"unknown Thrift type {ttype}")

    def struct(self, depth=1):
        if depth > self.max_depth:
            raise ParquetLimitError(f"Thrift nests more than {self.max_depth} deep")
        fields, fid = {}, 0
        while True:
            h = self.byte()
            if h == 0:
                return fields
            ttype = h & 0x0F
            fid = fid + (h >> 4) if h >> 4 else self.zigzag()
            fields[fid] = self.value(ttype, depth)


def check_footer(f, size):
    """Refuse a footer over 16 MiB or nested over 32 deep, before pyarrow parses it."""
    if size < 12:
        raise ParquetLimitError("is too short to be Parquet")
    f.seek(size - 8)
    tail = f.read(8)
    if tail[4:] != b"PAR1":
        raise ParquetLimitError("does not end with PAR1 (an encrypted or broken footer)")
    (n,) = struct.unpack("<I", tail[:4])
    if n > FOOTER_MAX or n > size - 12:
        raise ParquetLimitError(f"declares a footer of {n:,} bytes (the limit is {FOOTER_MAX:,})")
    f.seek(size - 8 - n)
    _Thrift(f.read(n)).struct()
    f.seek(0)


def page_totals(f, metadata):
    """(sum of pages' uncompressed sizes, data pages' values) from every page header."""
    if metadata.num_columns > MAX_COLUMNS:
        raise ParquetLimitError(f"has {metadata.num_columns:,} columns, over the limit of {MAX_COLUMNS:,}")
    uncompressed = values = 0
    for g in range(metadata.num_row_groups):
        rg = metadata.row_group(g)
        for c in range(rg.num_columns):
            col = rg.column(c)
            starts = [col.data_page_offset]
            if col.has_dictionary_page and col.dictionary_page_offset:
                starts.append(col.dictionary_page_offset)
            pos = min(starts)
            end = pos + col.total_compressed_size
            if pos < 0 or col.total_compressed_size < 0:
                raise ParquetLimitError("a column chunk has a negative offset or size")
            while pos < end:
                f.seek(pos)
                window = f.read(min(_HEADER_WINDOW, end - pos))
                t = _Thrift(window)
                header = t.struct()
                kind, unc, comp = header.get(1), header.get(2), header.get(3)
                if not all(isinstance(x, int) for x in (kind, unc, comp)) or unc < 0 or comp < 0:
                    raise ParquetLimitError(f"a page header in column {col.path_in_schema} is not readable")
                uncompressed += unc
                if kind == DATA_PAGE:
                    values += (header.get(5) or {}).get(1, 0)
                elif kind == DATA_PAGE_V2:
                    values += (header.get(8) or {}).get(1, 0)
                pos += t.pos + comp
    f.seek(0)
    return uncompressed, values
