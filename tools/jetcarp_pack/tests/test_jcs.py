"""RFC 8785 (JCS) against the RFC's own examples, the book's tiling keys, and (when the repository has
them) 600 vectors made by the independent Python `rfc8785` package."""

import json
import os
import struct
import unittest

from jetcarp_pack import jcs, safejson

# RFC 8785 Appendix B: IEEE 754 doubles (as hex) and their JSON text.
APPENDIX_B = """
0000000000000000 0
8000000000000000 0
0000000000000001 5e-324
8000000000000001 -5e-324
7fefffffffffffff 1.7976931348623157e+308
ffefffffffffffff -1.7976931348623157e+308
4340000000000000 9007199254740992
c340000000000000 -9007199254740992
4430000000000000 295147905179352830000
44b52d02c7e14af5 9.999999999999997e+22
44b52d02c7e14af6 1e+23
44b52d02c7e14af7 1.0000000000000001e+23
444b1ae4d6e2ef4e 999999999999999700000
444b1ae4d6e2ef4f 999999999999999900000
444b1ae4d6e2ef50 1e+21
3eb0c6f7a0b5ed8c 9.999999999999997e-7
3eb0c6f7a0b5ed8d 0.000001
41b3de4355555553 333333333.3333332
41b3de4355555554 333333333.33333325
41b3de4355555555 333333333.3333333
41b3de4355555556 333333333.3333334
41b3de4355555557 333333333.33333343
becbf647612f3696 -0.0000033333333333333333
43143ff3c1cb0959 1424953923781206.2
"""

VECTORS = os.path.join(os.path.dirname(__file__), "..", "..", "..", "crates", "jetcarp-core", "tests", "data",
                       "jcs_vectors.txt")


def double(hexbits):
    return struct.unpack(">d", bytes.fromhex(hexbits))[0]


class Numbers(unittest.TestCase):
    def test_appendix_b(self):
        for line in APPENDIX_B.strip().splitlines():
            bits, text = line.split()
            with self.subTest(bits=bits):
                self.assertEqual(jcs.number(double(bits)), text)

    def test_nan_and_infinity_have_no_form(self):
        for bits in ("7fffffffffffffff", "7ff0000000000000"):
            with self.assertRaises(jcs.CanonicalError):
                jcs.number(double(bits))

    def test_exponent_boundaries(self):
        cases = {1e-7: "1e-7", 1e-6: "0.000001", 1.5e-6: "0.0000015", 1e20: "100000000000000000000",
                 1e21: "1e+21", 123.0: "123", -0.0: "0", 0.1: "0.1", 2.0 ** 53: "9007199254740992"}
        for x, text in cases.items():
            self.assertEqual(jcs.number(x), text)

    def test_documents_hold_integers_only(self):
        self.assertEqual(jcs.dumps({"a": 2 ** 53}), '{"a":9007199254740992}')
        with self.assertRaises(jcs.CanonicalError):
            jcs.dumps({"a": 1.5})
        with self.assertRaises(jcs.CanonicalError):
            jcs.dumps({"a": 2 ** 53 + 1})
        self.assertEqual(jcs.dumps(2 ** 60, allow_floats=True), "1152921504606847000")


class Text(unittest.TestCase):
    def test_section_3_2_2_example(self):
        source = ('{"numbers": [333333333.33333329, 1E30, 4.50, 2e-3, 0.000000000000000000000000001], '
                  '"string": "\\u20ac$\\u000F\\u000aA\'\\u0042\\u0022\\u005c\\\\\\"\\/", '
                  '"literals": [null, true, false]}')
        value = safejson.loads(source, max_bytes=10_000, allow_floats=True)
        self.assertEqual(jcs.dumps(value, allow_floats=True),
                         '{"literals":[null,true,false],"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],'
                         '"string":"€$\\u000f\\nA\'B\\"\\\\\\\\\\"/"}')

    def test_section_3_2_3_sorting(self):
        source = ('{"\\u20ac": "Euro Sign", "\\r": "Carriage Return", "\\ufb33": "Hebrew Letter Dalet With Dagesh", '
                  '"1": "One", "\\ud83d\\ude00": "Emoji: Grinning Face", "\\u0080": "Control", '
                  '"\\u00f6": "Latin Small Letter O With Diaeresis"}')
        value = safejson.loads(source, max_bytes=10_000)
        keys = list(json.loads(jcs.dumps(value)))
        self.assertEqual(keys, ["\r", "1", "\u0080", "ö", "€", "\U0001f600", "דּ"])
        self.assertEqual(jcs.dumps(value)[:28], '{"\\r":"Carriage Return","1":')

    def test_utf16_order_differs_from_code_points(self):
        # U+1F600 (surrogates D83D DE00) sorts before U+FB33 by UTF-16 code units, after it by code point.
        self.assertEqual(jcs.dumps({"דּ": 1, "\U0001f600": 2}), '{"\U0001f600":2,"דּ":1}')

    def test_escapes(self):
        self.assertEqual(jcs.string("\x00\x1f\x7f \"\\/"), '"\\u0000\\u001f\x7f \\"\\\\/"')
        self.assertEqual(jcs.string("\b\f\n\r\t"), '"\\b\\f\\n\\r\\t"')

    def test_lone_surrogate_refused(self):
        with self.assertRaises(jcs.CanonicalError):
            jcs.string("\ud800")


class TilingKeys(unittest.TestCase):
    def test_book_examples(self):
        self.assertEqual(jcs.tiling_key({"family": "square"}), '{"family":"square","boundary":"dead"}')
        self.assertEqual(jcs.tiling_key({"family": "periodic", "params": {"catalog": "k3-019"}}),
                         '{"family":"periodic","params":{"catalog":"k3-019"},"boundary":"dead"}')
        self.assertEqual(jcs.tiling_key({"family": "spectre", "params": {"level": 3}}),
                         '{"family":"spectre","boundary":"dead"}')
        self.assertEqual(jcs.tiling_key({"family": "cairo", "params": {"b": 1.0, "a": [1e-7, 0.000001]}}),
                         '{"family":"cairo","params":{"a":[1e-7,0.000001],"b":1},"boundary":"dead"}')


@unittest.skipUnless(os.path.exists(VECTORS), "the repository's jcs_vectors.txt is not here")
class IndependentVectors(unittest.TestCase):
    """Vectors written by the Python rfc8785 package (tools/jcs_vectors.py), read as data."""

    def test_vectors(self):
        """Every value vector matches. Tiling vectors match the book's tiling_key except where an
        edge lacks `curved`: the vectors normalise each edge to {mode, curved (default true), nodes},
        which library-format.md does not say (SPEC_QUESTIONS.md, tiling key edges)."""
        n = edge_cases = 0
        with open(VECTORS, encoding="utf-8") as f:
            for line in f:
                if line.startswith("#") or not line.strip():
                    continue
                kind, source, expected = line.rstrip("\n").split("\t")
                value = json.loads(source)
                n += 1
                if kind == "value":
                    self.assertEqual(jcs.dumps(value, allow_floats=True), expected, source)
                    continue
                got = jcs.tiling_key(value)
                if got != expected:
                    self.assertTrue(any("curved" not in e for e in value.get("edges", [])), source)
                    value["edges"] = [{"mode": e["mode"], "curved": e.get("curved", True), "nodes": e["nodes"]}
                                      for e in value["edges"]]
                    self.assertEqual(jcs.tiling_key(value), expected, source)
                    edge_cases += 1
        self.assertGreater(n, 500)
        self.assertGreater(edge_cases, 0)


class SafeJson(unittest.TestCase):
    def load(self, text, **kw):
        return safejson.loads(text, max_bytes=kw.pop("max_bytes", 1 << 20), **kw)

    def test_deep_nesting_refused_without_recursion(self):
        with self.assertRaisesRegex(safejson.JsonError, "nests 100000 levels"):
            self.load("[" * 100_000 + "]" * 100_000, max_bytes=1 << 20)
        self.assertEqual(self.load("[" * 32 + "]" * 32), [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]])
        with self.assertRaises(safejson.JsonError):
            self.load("[" * 33 + "]" * 33)

    def test_brackets_in_strings_do_not_count(self):
        self.assertEqual(self.load('["' + "[" * 100 + '\\"' + '{' * 50 + '"]'), ["[" * 100 + '"' + "{" * 50])

    def test_refusals(self):
        for text, why in (('{"a":1,"a":2}', "repeats"), ("1.5", "non-integer"), ("1e5", "non-integer"),
                          ("NaN", "not JSON"), ("[Infinity]", "not JSON"), ("9007199254740993", "outside"),
                          ("-" + "9" * 30, "outside"), ('"\\ud800"', "lone surrogate"), ("[1,]", "not valid")):
            with self.subTest(text=text):
                with self.assertRaisesRegex(safejson.JsonError, why):
                    self.load(text)

    def test_bytes(self):
        with self.assertRaisesRegex(safejson.JsonError, "byte-order mark"):
            self.load(b"\xef\xbb\xbf{}")
        with self.assertRaisesRegex(safejson.JsonError, "not UTF-8"):
            self.load(b'"\xff"')
        with self.assertRaisesRegex(safejson.JsonError, "over the limit"):
            self.load(b"[" + b"1," * 600 + b"1]", max_bytes=1000)

    def test_canonical(self):
        self.assertTrue(safejson.is_canonical(b'{"a":1,"b":[]}', {"b": [], "a": 1}))
        self.assertFalse(safejson.is_canonical(b'{"b":[],"a":1}', {"b": [], "a": 1}))
        self.assertFalse(safejson.is_canonical(b'{"a": 1}', {"a": 1}))


if __name__ == "__main__":
    unittest.main()
