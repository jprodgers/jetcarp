"""Levels 0 (browse), 1 (integrity) and 2 (authenticity) on stores made by the test writer, and the
ways each must fail."""

from . import base  # noqa: F401  (first: without pyarrow, this module is skipped)

import json
import os
import unittest

from jetcarp_pack import browse, jcs
from jetcarp_pack.pack import Pack
from jetcarp_pack.report import Report, SKIP
from jetcarp_pack.store import Store, signer_name  # noqa: F401

from . import content as C
from . import stores
from .base import StoreTest, codes, current, fails, republish, run
from .packwriter import StoreWriter


class Browse(StoreTest):
    def test_tables_rows_and_find(self):
        out = stores.build_minimal(self.root(), self.keys)
        pack = Pack(Store(self.root()), out["manifest"])
        self.assertTrue(pack.ok)
        names = {t["table"]: t["rows"] for t in browse.tables(pack)}
        self.assertEqual(names["patterns"], 3)
        self.assertEqual(names["quickstart"], 1)
        rows = browse.rows(pack, "patterns", ["class=spaceship"], ["id", "period"])
        self.assertEqual(rows, [{"id": C.GLIDER_ID, "period": 4}])
        self.assertEqual(browse.find(pack, C.GLIDER_ID[:8])["record"]["id"], C.GLIDER_ID)
        self.assertEqual(browse.find(pack, "glider")["record"]["id"], C.GLIDER_ID)
        self.assertEqual(browse.find(pack, "5a0")["matches"], [])  # under 4 digits is no prefix
        ants = stores.build_ant_run(self.root("ants"), self.keys)
        pack = Pack(Store(self.root("ants")), ants["manifest"])
        ant = out["records"]["ant"]["id"]
        self.assertEqual(browse.find(pack, ant)["record"]["id"], ant)

    def test_retracted_record_is_shown_as_retracted(self):
        out = stores.build_retraction(self.root(), self.keys)
        pack = Pack(Store(self.root()), out["manifest"])
        found = browse.find(pack, out["records"]["blinker"]["id"])
        self.assertIn("Test retraction", found["retracted"]["reason"])
        self.assertEqual(found["retracted"]["by"], self.keys.a.fingerprint)
        glider = browse.find(pack, C.GLIDER_ID)
        self.assertNotIn("retracted", glider)
        # Latest wins by rev: version 2's name (rev 2, the same date order aside) over version 1's.
        self.assertEqual(glider["annotations_latest"], [{"publisher": self.keys.a.fingerprint, "field": "name",
                                                         "value": '"test glider"'}])


class Integrity(StoreTest):
    def test_minimal_is_clean(self):
        out = stores.build_minimal(self.root(), self.keys)
        self.assertEqual(out["records"]["glider"]["id"], C.GLIDER_ID)  # the book's id
        report = run(self.root(), 1)
        self.assertClean(report)
        self.assertEqual(report.passed[(1, "pattern content ids")], 3)
        self.assertEqual(report.passed[(1, "quickstart content ids")], 1)
        self.assertEqual(report.passed[(1, "pins recomputed from geometry")], 1)

    def test_ant_ids(self):
        stores.build_ant_run(self.root(), self.keys)
        report = run(self.root(), 1)
        self.assertClean(report)
        self.assertEqual(report.passed[(1, "ant content ids")], 1)

    def test_pin_must_follow_from_geometry(self):
        stores.build_bad_pin(self.root(), self.keys)
        self.assertIn("pin", fails(run(self.root(), 1)))

    def test_quickstart_id(self):
        def change(c):
            c["quickstart"][0]["title"] = "Renamed"  # not part of the id
            c["quickstart"][0]["scene"] = c["quickstart"][0]["scene"].replace("Test glider", "Other")
        self.assertIn("content-id", fails(self.mutated(change)))

    def test_json_cells_in_one_spelling(self):
        def change(c):
            c["annotations"][0]["value"] = '"glider" '
        self.assertIn("cell-not-canonical", fails(self.mutated(change)))

    def test_retractions_are_cumulative(self):
        stores.build_retraction(self.root(), self.keys)
        _, ref, _, _ = current(self.root())
        republish(self.root(), lambda d: d.update(parents=[ref["manifest"]], retracts=[]), self.keys.a)
        self.assertIn("retraction-dropped", fails(run(self.root(), 1)))

    def test_event_rows_are_not_added_twice(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        pack = Pack(Store(self.root()), out["manifest"])
        again = w.part("sightings", [dict(r) for _, _, r in pack.rows("sightings")][:1])
        index = w.index("sightings", out["entries"]["sightings"] + [again])
        republish(self.root(), lambda d: d.update(parents=[out["manifest"]], tables=dict(d["tables"], sightings=index)),
                  self.keys.a)
        self.assertIn("event-repeated", fails(run(self.root(), 1)))

    def test_tampered_object(self):
        stores.build_tampered_object(self.root(), self.keys)
        self.assertIn("object-hash", fails(run(self.root(), 1)))

    def test_manifest_not_canonical(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: None, self.keys.a,
                  raw=lambda d: json.dumps(d, indent=1).encode())
        self.assertIn("not-canonical", fails(run(self.root(), 1)))

    def test_unknown_requires(self):
        stores.build_unknown_requires(self.root(), self.keys)
        self.assertIn("unknown-requires", fails(run(self.root(), 1)))

    def test_unknown_major(self):
        stores.build_unknown_major(self.root(), self.keys)
        report = run(self.root(), 1)
        self.assertIn("unknown-major", fails(report))
        self.assertNotIn("manifest-field", fails(report))  # refused before any field is read

    def test_higher_minor_is_read(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d.update(version="1.7", future_field={"x": 1}), self.keys.a)
        report = run(self.root(), 1)
        self.assertClean(report)
        self.assertIn("unknown-field", codes(report, "INFO"))

    def test_unknown_schema_number(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        index = w.index("patterns", out["entries"]["patterns"], schema="jetcarp-patterns 2")
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertIn("unknown-schema", fails(run(self.root(), 1)))

    def mutated(self, change):
        def mutate(c):
            change(c)
        stores.build_minimal(self.root(), self.keys, mutate=mutate)
        return run(self.root(), 1)

    def glider(self, c):
        return next(p for p in c["patterns"] if p["class"] == "spaceship")

    def test_content_id(self):
        report = self.mutated(lambda c: self.glider(c).update(id="0" * 32))
        self.assertIn("content-id", fails(report))

    def test_identity_and_columns_disagree(self):
        report = self.mutated(lambda c: self.glider(c).update(dx=1))
        self.assertIn("identity-column", fails(report))

    def test_typed_phases_disagree(self):
        def change(c):
            self.glider(c)["phases"][1][0]["i"] += 5
        self.assertIn("identity-phases", fails(self.mutated(change)))

    def test_pattern_pin_differs_from_tiling(self):
        def change(c):
            g = self.glider(c)
            g["pin"] = "f" * 32
            g["id"] = C.ident.pattern_id(g["identity"], g["pin"])
        self.assertIn("record-pin", fails(self.mutated(change)))

    def test_tiling_key_spelling(self):
        def change(c):
            c["tilings"][0]["pin_key"] = '{"family": "square", "boundary": "dead"}'
        self.assertIn("tiling-key", fails(self.mutated(change)))

    def test_local_path(self):
        def change(c):
            c["provenance"][0]["source"] = "/home/someone/scenes/glider.json"
        self.assertIn("local-path", fails(self.mutated(change)))

    def test_fact_conflict(self):
        """Promise 4: two parts with the same pattern must hold identical rows."""
        out = stores.build_minimal(self.root(), self.keys)
        g = dict(out["records"]["glider"], speed="c/4 (another spelling)")
        w = StoreWriter(self.root(), marker=False)
        extra = w.part("patterns", [g])
        index = w.index("patterns", out["entries"]["patterns"] + [extra])
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertIn("fact-conflict", fails(run(self.root(), 1)))

    def test_index_entry_disagrees_with_part(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        e = [dict(x) for x in out["entries"]["patterns"]]
        e[0]["rows"] += 1
        e[1]["bytes"] += 1
        e[2]["first"] = "0" * 32
        index = w.index("patterns", e)
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        f = fails(run(self.root(), 1))
        self.assertTrue({"part-rows", "part-bytes", "part-range"} <= f, f)

    def test_partition_disagrees(self):
        out = stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        e = [dict(x) for x in out["entries"]["patterns"]]
        e[0]["partition"] = {"class_group": "gun", "tiling_group": "square"}
        index = w.index("patterns", e)
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertIn("part-partition", fails(run(self.root(), 1)))

    def test_unsorted_part(self):
        out = stores.build_minimal(self.root(), self.keys)
        import pyarrow as pa
        import pyarrow.parquet as pq
        from jetcarp_pack import schemas
        from .packwriter import set_created_by
        rows = sorted([out["records"]["glider"], out["records"]["block"]], key=lambda r: r["id"], reverse=True)
        spec = schemas.TABLES["patterns"]
        table = pa.Table.from_pylist([dict(r, class_group="x") for r in rows], schema=spec.arrow_schema())
        sink = pa.BufferOutputStream()
        pq.write_table(table, sink, compression="zstd", store_schema=False)
        data = set_created_by(sink.getvalue().to_pybytes(), "jetcarp-pack 1")
        w = StoreWriter(self.root(), marker=False)
        h = w.put(data, "parquet")
        index = w.index("patterns", [{"object": h, "rows": 2, "bytes": len(data),
                                      "partition": {"class_group": "x", "tiling_group": "square"},
                                      "first": rows[0]["id"], "last": rows[1]["id"]}])
        republish(self.root(), lambda d: d["tables"].update(patterns=index), self.keys.a)
        self.assertIn("part-sort", fails(run(self.root(), 1)))

    def test_retained_version_must_be_kept(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d.update(retain=["sha256:" + "1" * 64]), self.keys.a)
        self.assertIn("retain-missing", fails(run(self.root(), 1)))

    def test_media_frames_are_refused(self):
        stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        png = w.put(b"\x89PNG not really", "png")
        part = w.part("media", [{"target": f"pattern:{C.GLIDER_ID}", "kind": "frames", "from_seed": False,
                                 "object": png, "media_type": "image/png", "width": 8, "height": 8, "frames": 1,
                                 "bytes": 15, "render_key": "k", "render": "{}"}])
        index = w.index("media", [part])
        republish(self.root(), lambda d: d["tables"].update(media=index), self.keys.a)
        f = fails(run(self.root(), 1))
        self.assertIn("media-kind", f)
        self.assertNotIn("object-hash", f)


class Exports(StoreTest):
    def test_export_is_clean(self):
        stores.build_export(self.root(), self.keys)
        report = run(self.root(), 1)
        self.assertClean(report)
        self.assertGreater(report.passed[(1, "export files in their places")], 5)
        self.assertClean(run(self.root(), 2))
        self.assertEqual(run(self.root(), 3).passed[(3, "patterns replayed")], 3)

    def test_export_layout(self):
        stores.build_export(self.root(), self.keys)
        folder = os.path.join(self.root(), "tables", "patterns")
        a = os.path.join(folder, "class_group=spaceship", "tiling_group=square", "part-000001.parquet")
        b = os.path.join(folder, "class_group=still_life", "tiling_group=square", "part-000001.parquet")
        tmp = a + ".swap"
        os.rename(a, tmp)
        os.rename(b, a)
        os.rename(tmp, b)
        self.assertIn("export-layout", fails(run(self.root(), 1)))


class Ratings(StoreTest):
    def build(self, change):
        out = stores.build_group_raters(self.root(), self.keys)
        _, _, _, doc = current(self.root())
        store = Store(self.root())
        pack = Pack(store, out["manifest"])
        tables = {n: [dict(r) for _, _, r in pack.rows(n)] for n in ("raters", "ratings", "score_models")}
        change(tables)
        w = StoreWriter(self.root(), marker=False)
        index, _ = w.tables(tables)
        republish(self.root(), lambda d: d.update(tables=index), self.keys.a)
        return run(self.root(), 1)

    def test_group_raters_are_clean(self):
        stores.build_group_raters(self.root(), self.keys)
        self.assertClean(run(self.root(), 1))

    def test_rating_without_consent(self):
        report = self.build(lambda t: t["raters"].pop(1))  # RATER_2 has no row now
        self.assertIn("rater-consent", fails(report))
        self.assertIn("rater-group", fails(report))  # and the group's member is missing

    def test_group_has_no_name_of_its_own(self):
        def change(t):
            next(r for r in t["raters"] if r["members"])["display_name"] = "Test Group"
        self.assertIn("rater-group", fails(self.build(change)))

    def test_consent_scope(self):
        def change(t):
            t["raters"][0]["consent_scope"] = "private"
        self.assertIn("rater-consent", fails(self.build(change)))


class Authenticity(StoreTest):
    def test_minimal_is_clean(self):
        stores.build_minimal(self.root(), self.keys)
        report = run(self.root(), 2)
        self.assertClean(report)
        self.assertEqual(report.passed[(2, "manifest signatures")], 1)
        self.assertEqual(report.passed[(2, "ref signatures")], 1)

    def test_bad_signature(self):
        stores.build_bad_signature(self.root(), self.keys)
        self.assertIn("bad-signature", fails(run(self.root(), 2)))

    def test_signature_by_another_key_under_the_publishers_name(self):
        out = stores.build_minimal(self.root(), self.keys)
        h, data = out["manifest"], out["data"]
        sig = self.keys.b.sign("jetcarp-manifest", data)
        hexd = h[7:]
        StoreWriter(self.root(), marker=False).write(
            ("sigs", "sha256", hexd[:2], hexd, signer_name(self.keys.a.fingerprint) + ".sig"), sig.encode())
        report = run(self.root(), 2)
        self.assertIn("sig-name", fails(report))  # the file's <signer> is not the signature's key
        self.assertTrue(any("named for another key" in f.message for f in report.findings))

    def test_wrong_namespace(self):
        out = stores.build_minimal(self.root(), self.keys)
        h, data = out["manifest"], out["data"]
        sig = self.keys.a.sign("jetcarp-ref", data)
        hexd = h[7:]
        StoreWriter(self.root(), marker=False).write(
            ("sigs", "sha256", hexd[:2], hexd, signer_name(self.keys.a.fingerprint) + ".sig"), sig.encode())
        self.assertIn("bad-signature", fails(run(self.root(), 2)))

    def test_unsigned_manifest(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d.update(title="Unsigned"), self.keys.a, sign=False)
        self.assertIn("unsigned", fails(run(self.root(), 2)))

    def test_tampered_ref(self):
        stores.build_minimal(self.root(), self.keys)
        feed, ref, _, _ = current(self.root())
        ref = dict(ref, seq=ref["seq"] + 5)
        StoreWriter(self.root(), marker=False).write(("refs", f"{feed}.json"), jcs.canonical(ref))
        self.assertIn("bad-signature", fails(run(self.root(), 2)))

    def test_ref_seq_rollback(self):
        stores.build_retraction(self.root("new"), self.keys)
        stores.build_rollback(self.root("old"), self.keys)
        state = os.path.join(self.tmp, "state.json")
        self.assertClean(run(self.root("new"), 2, state=state, accept=True))
        with open(state) as f:
            self.assertEqual(next(iter(json.load(f)["feeds"].values()))["seq"], 2)
        self.assertIn("seq-rollback", fails(run(self.root("old"), 2, state=state)))
        # The same seq again for the same manifest is no update, not a failure.
        report = run(self.root("new"), 2, state=state)
        self.assertClean(report)
        self.assertIn("seq-unchanged", codes(report, "INFO"))

    def test_ref_seq_reused_for_another_manifest(self):
        stores.build_minimal(self.root(), self.keys)
        state = os.path.join(self.tmp, "state.json")
        self.assertClean(run(self.root(), 2, state=state, accept=True))
        republish(self.root(), lambda d: d.update(title="Other"), self.keys.a, seq=1)
        self.assertIn("seq-reused", fails(run(self.root(), 2, state=state)))

    def test_ref_seq_must_be_positive(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: None, self.keys.a, seq=0)
        self.assertIn("ref-seq", fails(run(self.root(), 2)))

    def test_ref_and_manifest_disagree(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d.update(feed="another-feed"), self.keys.a)
        self.assertIn("ref-manifest", fails(run(self.root(), 2)))

    def test_revoked_key(self):
        stores.build_minimal(self.root(), self.keys)
        StoreWriter(self.root(), marker=False).key_statement(self.keys.a, stores.D0, revoked=True,
                                                             name="Test Publisher A")
        f = fails(run(self.root(), 2))
        self.assertIn("revoked-key", f)

    def test_revocation_ignores_dates(self):
        out = stores.build_minimal(self.root(), self.keys)
        StoreWriter(self.root(), marker=False).key_statement(self.keys.a, stores.D3, revoked=True)
        report = run(self.root(), 2, manifest=out["manifest"])
        self.assertIn("revoked-key", fails(report))  # made before the revocation, still refused

    def test_revocation_is_kept(self):
        """A store that serves the old statement again cannot undo a revocation already seen."""
        stores.build_minimal(self.root(), self.keys)
        state = os.path.join(self.tmp, "state.json")
        self.assertClean(run(self.root(), 2, state=state, accept=True))
        w = StoreWriter(self.root(), marker=False)
        statement = os.path.join(self.root(), "keys", signer_name(self.keys.a.fingerprint) + ".json")
        with open(statement, "rb") as f:
            good = f.read()
        w.key_statement(self.keys.a, stores.D2, revoked=True)
        self.assertIn("revoked-key", fails(run(self.root(), 2, state=state, accept=True)))
        with open(statement, "wb") as f:
            f.write(good)
        report = run(self.root(), 2, state=state)
        self.assertIn("revoked-key", fails(report))
        self.assertIn("revocation-kept", codes(report, "WARN"))

    def test_machine_key_revoked_in_place(self):
        stores.build_revoked_machine_key(self.root(), self.keys)
        self.assertIn("no-publisher-signature", fails(run(self.root(), 2)))

    def test_successor_after_revocation(self):
        stores.build_revoked_by_successor(self.root("by-successor"), self.keys)
        self.assertClean(run(self.root("by-successor"), 2))
        stores.build_successor_of_self_revoked(self.root("self"), self.keys)
        self.assertIn("revoked-key", fails(run(self.root("self"), 2)))

    def test_machine_key_must_be_authorised(self):
        stores.build_minimal(self.root(), self.keys)
        republish(self.root(), lambda d: d.update(title="From the laptop"), self.keys.a_laptop,
                  ref_key=self.keys.a)
        report = run(self.root(), 2)
        self.assertIn("no-publisher-signature", fails(report))
        # Once A's statement lists the laptop key, the same signature is A's.
        StoreWriter(self.root(), marker=False).key_statement(self.keys.a, stores.D0, name="Test Publisher A",
                                                             authorised=[(self.keys.a_laptop, "laptop")])
        self.assertClean(run(self.root(), 2))

    def test_merge_signed_on_machine_key(self):
        stores.build_merge(self.root(), self.keys)
        report = run(self.root(), 2)
        self.assertClean(report)
        self.assertEqual(report.passed[(2, "manifest signatures")], 3)

    def test_key_rotation(self):
        stores.build_key_rotation(self.root(), self.keys)
        self.assertClean(run(self.root(), 2))

    def test_ref_must_be_canonical(self):
        stores.build_minimal(self.root(), self.keys)
        feed, ref, _, _ = current(self.root())
        StoreWriter(self.root(), marker=False).write(("refs", f"{feed}.json"), json.dumps(ref).encode())
        self.assertIn("not-canonical", fails(run(self.root(), 2)))

    def test_ref_by_superseded_key(self):
        out = stores.build_key_rotation(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        w.ref("test-rotation", 3, out["manifest"], self.keys.r1.fingerprint, stores.D3, self.keys.r1)
        self.assertIn("superseded-key", fails(run(self.root(), 2)))

    def test_unconfirmed_supersession(self):
        stores.build_minimal(self.root(), self.keys)
        StoreWriter(self.root(), marker=False).key_statement(self.keys.b, stores.D1, supersedes=self.keys.a.fingerprint,
                                                             github="test-publisher-b")
        report = run(self.root(), 2)
        skips = [f for f in report.findings if f.severity == SKIP and f.code == "key-unconfirmed"]
        self.assertTrue(skips and "github.com/test-publisher-b.keys" in skips[0].message)

    def test_two_successors(self):
        stores.build_key_rotation(self.root(), self.keys)
        StoreWriter(self.root(), marker=False).key_statement(self.keys.b, stores.D2, signer=self.keys.r1,
                                                             supersedes=self.keys.r1.fingerprint)
        self.assertIn("key-fork", fails(run(self.root(), 2)))

    def test_key_statement_under_another_name(self):
        stores.build_minimal(self.root(), self.keys)
        w = StoreWriter(self.root(), marker=False)
        doc = w.key_statement(self.keys.b, stores.D0)
        os.remove(os.path.join(self.root(), "keys", signer_name(self.keys.b.fingerprint) + ".json"))
        w.write(("keys", signer_name(self.keys.a.fingerprint) + ".json"), jcs.canonical(doc))
        f = fails(run(self.root(), 2))
        self.assertIn("key-fingerprint", f)

    def test_endorsement(self):
        out = stores.build_merge(self.root(), self.keys)
        h = out["manifest"]
        _, _, data, _ = current(self.root(), "test-a")
        hexd = h[7:]
        StoreWriter(self.root(), marker=False).write(
            ("sigs", "sha256", hexd[:2], hexd, signer_name(self.keys.b.fingerprint) + ".sig"),
            self.keys.b.sign("jetcarp-manifest", data).encode())
        report = run(self.root(), 2)
        self.assertClean(report)
        self.assertIn("endorsement", codes(report, "INFO"))


if __name__ == "__main__":
    unittest.main()
