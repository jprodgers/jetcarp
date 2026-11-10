//! Stage 1 settings and score models in the catalog (tables `settings`, `score_models`).
//!
//! A setting's status follows its score and the active score model's thresholds: `auto` (shown
//! in the Quick Start without review), `candidate` (waits in the approval queue), or `stored`
//! (kept, not queued). `approved` and `rejected` are the user's decisions and never change on a
//! rescore. Settings that are `auto`, `candidate`, or `approved` have a Quick Start row
//! `setting-<id>` with the same status (a scene that reproduces soup 0 exactly); a setting that
//! drops to `stored` loses its row unless the user decided on it.

use super::*;
use crate::library::tiling_display_name;
use crate::stage1::eval::{Descriptors, Setting, boundary_name};
use crate::stage1::score::{Sample, ScoreModel, features, refit};

/// How [`Library::thin_quickstart`] thins the Quick Start's search picks and the queue.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThinOptions {
    pub auto_per_space: usize,
    pub candidate_per_space: usize,
    /// The same on squares and hexagons (the main grids, with every rule searched).
    pub auto_main: usize,
    pub candidate_main: usize,
    /// Novelty-vector distance below which two settings count as look-alikes.
    pub min_distance: f64,
    pub dry_run: bool,
}

impl Default for ThinOptions {
    fn default() -> Self {
        ThinOptions {
            auto_per_space: 6,
            candidate_per_space: 3,
            auto_main: 24,
            candidate_main: 12,
            min_distance: 0.06,
            dry_run: false,
        }
    }
}

/// What a thinning did (statuses of the undecided settings before and after).
#[derive(Clone, Debug, Default, Serialize)]
pub struct ThinReport {
    pub before: BTreeMap<String, usize>,
    pub after: BTreeMap<String, usize>,
    pub look_alikes: usize,
    pub spaces: usize,
    pub changed: usize,
}

/// Status of a stage 1 setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingStatus {
    /// Below the candidate threshold: kept, not queued.
    Stored,
    /// Waits in the approval queue.
    Candidate,
    /// Above the auto threshold: in the Quick Start without review.
    Auto,
    Approved,
    Rejected,
}

impl SettingStatus {
    pub fn name(self) -> String {
        name_of(&self)
    }
    pub fn parse(s: &str) -> Result<SettingStatus, String> {
        parse_name(&s.trim().to_ascii_lowercase()).map_err(|_| {
            format!("unknown status '{s}' (stored, candidate, auto, approved, rejected)")
        })
    }
    /// The user decided (a rescore leaves it alone).
    pub fn decided(self) -> bool {
        matches!(self, SettingStatus::Approved | SettingStatus::Rejected)
    }
    /// The Quick Start status of its entry, if it has one.
    pub fn quickstart(self) -> Option<QsStatus> {
        match self {
            SettingStatus::Auto => Some(QsStatus::Auto),
            SettingStatus::Candidate => Some(QsStatus::Candidate),
            SettingStatus::Approved => Some(QsStatus::Approved),
            SettingStatus::Rejected => Some(QsStatus::Rejected),
            SettingStatus::Stored => None,
        }
    }
    /// The status a score gives under a model.
    pub fn for_score(m: &ScoreModel, score: f64) -> SettingStatus {
        match m.status_for(score) {
            "auto" => SettingStatus::Auto,
            "candidate" => SettingStatus::Candidate,
            _ => SettingStatus::Stored,
        }
    }
}

/// One row of `settings`.
#[derive(Clone, Debug, Serialize)]
pub struct SettingRow {
    pub id: i64,
    pub setting: Setting,
    /// The CA notation (`B2/S34V` for edge neighbours) and the rule's family and bitmasks.
    pub notation: String,
    pub family: String,
    pub states: u32,
    pub birth: Option<u32>,
    pub survive: Option<u32>,
    pub tiling_name: String,
    pub descriptors: Descriptors,
    pub score: Option<f64>,
    pub status: SettingStatus,
    pub model_id: Option<i64>,
    /// `coarse`, `refine`, `neighbours`, ...
    pub pass: String,
    pub behaviour: String,
    pub job: Option<i64>,
    pub created: String,
    pub updated: String,
}

impl SettingRow {
    /// The Quick Start entry id of this setting.
    pub fn entry_id(&self) -> String {
        format!("setting-{}", self.id)
    }
}

/// Filters for [`Library::settings`]. Empty fields match everything.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SettingsQuery {
    #[serde(default)]
    pub status: Vec<SettingStatus>,
    #[serde(default)]
    pub job: Option<i64>,
    /// Tiling family id (`hex`) or display name.
    #[serde(default)]
    pub tiling: Option<String>,
    /// Rule notation, exactly as stored (`B3/S23`).
    #[serde(default)]
    pub rule: Option<String>,
    /// Rule family (`lifelike`, `generations`).
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub behaviour: Option<String>,
    #[serde(default)]
    pub min_score: Option<f64>,
    /// `score` (default, best first), `created` (newest first), or `id`.
    #[serde(default)]
    pub order: Option<String>,
    /// 0 = no limit.
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

/// A setting to store (see [`Library::stage1_flush`]).
#[derive(Clone, Debug)]
pub struct NewSetting {
    pub setting: Setting,
    pub descriptors: Descriptors,
    pub score: f64,
    pub status: SettingStatus,
    pub model_id: Option<i64>,
    pub pass: String,
}

/// Builds a job's progress and checkpoint from what a flush wrote.
pub type FlushFinish<'a> = dyn Fn(&Stage1Flush) -> Result<(Value, Vec<u8>), String> + 'a;

/// What a flush wrote.
#[derive(Clone, Debug, Default)]
pub struct Stage1Flush {
    /// Setting ids, one per row, in order.
    pub ids: Vec<i64>,
    /// Final statuses (decided settings keep theirs).
    pub statuses: Vec<SettingStatus>,
}

/// What [`Library::rescore`] changed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RescoreReport {
    pub model_id: i64,
    pub settings: usize,
    /// Settings whose status changed, by new status.
    pub changed: BTreeMap<String, usize>,
    /// Statuses after the rescore.
    pub statuses: BTreeMap<String, usize>,
}

use std::collections::BTreeMap;

const SCOLS: &str = "s.id, s.spec, r.notation, r.neighborhood, r.family, r.states, r.birth, \
    r.survive, s.tiling_name, s.descriptors, s.score, s.qs_status, s.model_id, s.pass, \
    s.behaviour, s.job_id, s.created, s.updated";

fn setting_from_row(row: &rusqlite::Row) -> rusqlite::Result<Result<SettingRow, String>> {
    let spec: String = row.get(1)?;
    let nb: String = row.get(3)?;
    let desc: String = row.get(9)?;
    let status: String = row.get(11)?;
    let notation: String = row.get(2)?;
    let out = || -> Result<SettingRow, String> {
        let nb: Nb = parse_name(&nb)?;
        let notation = if nb == Nb::Edge && !notation.ends_with('V') {
            format!("{notation}V")
        } else {
            notation.clone()
        };
        Ok(SettingRow {
            id: row.get(0).map_err(e)?,
            setting: serde_json::from_str(&spec).map_err(|x| format!("setting spec: {x}"))?,
            notation,
            family: row.get(4).map_err(e)?,
            states: row.get::<_, i64>(5).map_err(e)? as u32,
            birth: row.get::<_, Option<i64>>(6).map_err(e)?.map(|x| x as u32),
            survive: row.get::<_, Option<i64>>(7).map_err(e)?.map(|x| x as u32),
            tiling_name: row.get(8).map_err(e)?,
            descriptors: serde_json::from_str(&desc).unwrap_or_default(),
            score: row.get(10).map_err(e)?,
            status: parse_name(&status)?,
            model_id: row.get(12).map_err(e)?,
            pass: row.get(13).map_err(e)?,
            behaviour: row.get(14).map_err(e)?,
            job: row.get(15).map_err(e)?,
            created: row.get(16).map_err(e)?,
            updated: row.get(17).map_err(e)?,
        })
    };
    Ok(out())
}

/// Text forms of a setting's key columns.
fn key_cols(s: &Setting) -> (String, String, String, String) {
    (
        tiling_key(&s.tiling),
        format!("{}", s.soup),
        format!("{}x{}", s.grid[0], s.grid[1]),
        boundary_name(s.boundary).to_string(),
    )
}

/// Write (or replace, or remove) the Quick Start row of a setting to match its status.
fn sync_entry(
    tx: &Transaction,
    id: i64,
    s: &Setting,
    d: &Descriptors,
    score: Option<f64>,
    status: SettingStatus,
) -> Result<(), String> {
    let entry = format!("setting-{id}");
    match status.quickstart() {
        Some(qs) => {
            let mut q = crate::stage1::job::quickstart_entry(id, s, d, score.unwrap_or(0.0))?;
            q.status = qs;
            let created: Option<String> = tx
                .query_row(
                    "SELECT created FROM quickstart WHERE id = ?1",
                    [&entry],
                    |r| r.get(0),
                )
                .optional()
                .map_err(e)?;
            if let Some(c) = created {
                q.created = c;
            }
            quickstart_in(tx, &q)
        }
        None => {
            tx.execute("DELETE FROM quickstart WHERE id = ?1", [&entry])
                .map_err(e)?;
            Ok(())
        }
    }
}

impl Library {
    /// Store evaluated settings and the job's progress and checkpoint in one transaction. A
    /// setting already stored (same rule, tiling, density, soup, grid, boundary, generations)
    /// is updated in place; a decided one (approved or rejected) keeps its status. Quick Start
    /// rows follow the statuses. `finish`, given the ids and final statuses, returns the job's
    /// progress and checkpoint, written in the same transaction.
    pub fn stage1_flush(
        &self,
        job: Option<i64>,
        rows: &[NewSetting],
        finish: Option<&FlushFinish<'_>>,
    ) -> Result<Stage1Flush, String> {
        let now = now_utc();
        self.write(|tx| {
            let mut out = Stage1Flush::default();
            for r in rows {
                let s = &r.setting;
                let (rule, nb) = s.rule()?;
                let rule_id = rule_id_in(tx, &rule.notation(), nb, 1)?;
                let (tkey, soup, grid, boundary) = key_cols(s);
                let existing: Option<(i64, String)> = tx
                    .prepare_cached(
                        "SELECT id, qs_status FROM settings WHERE rule_id = ?1 AND tiling = ?2
                         AND density = ?3 AND soup_size = ?4 AND grid_size = ?5 AND boundary = ?6
                         AND generations = ?7",
                    )
                    .map_err(e)?
                    .query_row(
                        params![
                            rule_id,
                            tkey,
                            s.density,
                            soup,
                            grid,
                            boundary,
                            s.gens as i64
                        ],
                        |x| Ok((x.get(0)?, x.get(1)?)),
                    )
                    .optional()
                    .map_err(e)?;
                let spec = serde_json::to_string(s).map_err(|x| x.to_string())?;
                let desc = serde_json::to_string(&r.descriptors).map_err(|x| x.to_string())?;
                let had = existing.is_some();
                let (id, status) = match existing {
                    Some((id, old)) => {
                        let old: SettingStatus = parse_name(&old)?;
                        let status = if old.decided() { old } else { r.status };
                        tx.execute(
                            "UPDATE settings SET soups = ?2, seed = ?3, spec = ?4,
                                descriptors = ?5, score = ?6, qs_status = ?7, model_id = ?8,
                                pass = ?9, behaviour = ?10, job_id = ?11, updated = ?12,
                                tiling_name = ?13
                             WHERE id = ?1",
                            params![
                                id,
                                s.soups as i64,
                                s.seed as i64,
                                spec,
                                desc,
                                r.score,
                                status.name(),
                                r.model_id,
                                r.pass,
                                r.descriptors.behaviour,
                                job,
                                now,
                                tiling_display_name(&s.tiling),
                            ],
                        )
                        .map_err(e)?;
                        (id, status)
                    }
                    None => {
                        tx.prepare_cached(
                            "INSERT INTO settings (rule_id, tiling, tiling_name, density,
                                soup_size, grid_size, boundary, generations, soups, seed, spec,
                                descriptors, score, qs_status, model_id, pass, behaviour, job_id,
                                created, updated)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                                ?15, ?16, ?17, ?18, ?19, ?19)",
                        )
                        .map_err(e)?
                        .execute(params![
                            rule_id,
                            tkey,
                            tiling_display_name(&s.tiling),
                            s.density,
                            soup,
                            grid,
                            boundary,
                            s.gens as i64,
                            s.soups as i64,
                            s.seed as i64,
                            spec,
                            desc,
                            r.score,
                            r.status.name(),
                            r.model_id,
                            r.pass,
                            r.descriptors.behaviour,
                            job,
                            now,
                        ])
                        .map_err(e)?;
                        (tx.last_insert_rowid(), r.status)
                    }
                };
                // Quick Start rows only for settings worth showing or queueing; a stored
                // setting that never had one costs no query.
                if status != SettingStatus::Stored || had {
                    sync_entry(tx, id, s, &r.descriptors, Some(r.score), status)?;
                }
                out.ids.push(id);
                out.statuses.push(status);
            }
            if let Some(job) = job
                && let Some(f) = finish
            {
                let (progress, checkpoint) = f(&out)?;
                let n = tx
                    .execute(
                        "UPDATE jobs SET updated = ?2, progress = ?3, checkpoint = ?4
                         WHERE id = ?1",
                        params![job, now, progress.to_string(), checkpoint],
                    )
                    .map_err(e)?;
                if n == 0 {
                    return Err(format!("no job {job}"));
                }
            }
            Ok(out)
        })
    }

    /// Settings matching a query.
    pub fn settings(&self, q: &SettingsQuery) -> Result<Vec<SettingRow>, String> {
        let (filter, args) = settings_where(q);
        let order = match q.order.as_deref().unwrap_or("score") {
            "score" => "s.score IS NULL, s.score DESC, s.id",
            "created" => "s.id DESC",
            "id" => "s.id",
            x => return Err(format!("unknown order '{x}' (score, created, id)")),
        };
        let limit = if q.limit == 0 {
            String::new()
        } else {
            format!(" LIMIT {} OFFSET {}", q.limit, q.offset)
        };
        self.read(vec![], |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT {SCOLS} FROM settings s JOIN rules r ON r.id = s.rule_id
                     {filter} ORDER BY {order}{limit}"
                ))
                .map_err(e)?;
            let rows = st
                .query_map(rusqlite::params_from_iter(args.iter()), setting_from_row)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            rows.into_iter().collect()
        })
    }

    /// How many settings match a query (ignoring its limit), by status.
    pub fn settings_counts(&self, q: &SettingsQuery) -> Result<BTreeMap<String, u64>, String> {
        let (filter, args) = settings_where(q);
        self.read(BTreeMap::new(), |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT s.qs_status, count(*) FROM settings s JOIN rules r ON r.id = s.rule_id
                     {filter} GROUP BY s.qs_status"
                ))
                .map_err(e)?;
            let rows = st
                .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                })
                .map_err(e)?
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map_err(e)?;
            Ok(rows)
        })
    }

    /// One setting by id (`12`, `setting-12`, or `find:setting-12`).
    pub fn setting(&self, id: &str) -> Result<SettingRow, String> {
        let n = parse_setting_id(id)?;
        self.read(None, |c| {
            c.query_row(
                &format!(
                    "SELECT {SCOLS} FROM settings s JOIN rules r ON r.id = s.rule_id
                     WHERE s.id = ?1"
                ),
                [n],
                setting_from_row,
            )
            .optional()
            .map_err(e)
        })?
        .ok_or_else(|| format!("no setting {n}"))?
    }

    /// Approve or reject a setting (or set any status). Its Quick Start row follows. An approval
    /// also counts as a user rating (`rating`, default 4); a rejection is rated only when a
    /// rating is given.
    pub fn review_setting(
        &self,
        id: &str,
        status: SettingStatus,
        rating: Option<f64>,
        note: &str,
    ) -> Result<SettingRow, String> {
        let row = self.setting(id)?;
        let rating = match status {
            SettingStatus::Approved => Some(rating.unwrap_or(4.0)),
            _ => rating,
        };
        let now = now_utc();
        self.write(|tx| {
            tx.execute(
                "UPDATE settings SET qs_status = ?2, updated = ?3 WHERE id = ?1",
                params![row.id, status.name(), now],
            )
            .map_err(e)?;
            sync_entry(
                tx,
                row.id,
                &row.setting,
                &row.descriptors,
                row.score,
                status,
            )?;
            if let Some(r) = rating {
                super::ratings::rate_in(tx, "setting", &row.id.to_string(), "user", r, note, &now)?;
            }
            Ok(())
        })?;
        self.setting(&row.id.to_string())
    }

    /// The active score model for settings (the hand-set one when none is stored).
    pub fn score_model(&self) -> Result<ScoreModel, String> {
        Ok(self
            .score_models()?
            .into_iter()
            .rev()
            .find(|(_, active)| *active)
            .map(|x| x.0)
            .unwrap_or_else(ScoreModel::hand))
    }

    /// Every stored score model, oldest first, with whether it is active.
    pub fn score_models(&self) -> Result<Vec<(ScoreModel, bool)>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT id, model, active, created FROM score_models
                     WHERE target = 'setting' ORDER BY id",
                )
                .map_err(e)?;
            let rows = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            rows.into_iter()
                .map(|(id, m, active, created)| {
                    let mut m: ScoreModel =
                        serde_json::from_str(&m).map_err(|x| format!("score model {id}: {x}"))?;
                    m.id = id;
                    m.created = created;
                    Ok((m, active))
                })
                .collect()
        })
    }

    /// Store a score model as the active one (the others become inactive). Returns its id.
    pub fn put_score_model(&self, m: &ScoreModel) -> Result<i64, String> {
        let now = now_utc();
        let json = serde_json::to_string(m).map_err(|x| x.to_string())?;
        self.write(|tx| {
            tx.execute(
                "UPDATE score_models SET active = 0 WHERE target = 'setting'",
                [],
            )
            .map_err(e)?;
            tx.execute(
                "INSERT INTO score_models (target, source, model, ratings, active, created)
                 VALUES ('setting', ?1, ?2, ?3, 1, ?4)",
                params![m.source, json, m.ratings as i64, now],
            )
            .map_err(e)?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// The active model, stored first if it is the built-in hand-set one (so settings can
    /// refer to it by id).
    pub fn ensure_score_model(&self) -> Result<ScoreModel, String> {
        let m = self.score_model()?;
        if m.id != 0 {
            return Ok(m);
        }
        let id = self.put_score_model(&m)?;
        self.score_model().map(|mut x| {
            x.id = id;
            x
        })
    }

    /// Rating samples for a refit: every user and model rating of a setting with its features.
    /// Returns the samples and the number of distinct rated settings.
    pub fn rating_samples(&self) -> Result<(Vec<Sample>, usize), String> {
        let ratings = self.ratings(Some("setting"))?;
        let mut cache: HashMap<i64, Option<[f64; 17]>> = HashMap::new();
        let mut samples = Vec::new();
        for r in &ratings {
            let Ok(id) = r.target_id.parse::<i64>() else {
                continue;
            };
            let f = match cache.get(&id) {
                Some(f) => *f,
                None => {
                    let f = self
                        .setting(&id.to_string())
                        .ok()
                        .map(|s| features(&s.descriptors, s.setting.soup < 1.0));
                    cache.insert(id, f);
                    f
                }
            };
            if let Some(f) = f {
                samples.push(Sample {
                    features: f,
                    rating: r.rating,
                    weight: if r.rater == "user" { 1.0 } else { 0.5 },
                });
            }
        }
        let rated = cache.values().filter(|f| f.is_some()).count();
        Ok((samples, rated))
    }

    /// Refit the score from the ratings, store the new model as active, and rescore every
    /// setting. Refuses with fewer than `score::MIN_RATED` rated settings.
    pub fn refit_score(&self, lambda: f64) -> Result<(ScoreModel, RescoreReport), String> {
        let prior = self.score_model()?;
        let (samples, rated) = self.rating_samples()?;
        let mut m = refit(&prior, &samples, rated, lambda)?;
        m.id = self.put_score_model(&m)?;
        let rep = self.rescore(&m)?;
        Ok((m, rep))
    }

    /// New thresholds: a copy of the active model with them, stored as active, and a rescore.
    pub fn set_thresholds(
        &self,
        auto: f64,
        candidate: f64,
    ) -> Result<(ScoreModel, RescoreReport), String> {
        if candidate > auto {
            return Err("the candidate threshold must not be above the auto threshold".into());
        }
        let mut m = self.score_model()?;
        m.source = "thresholds".into();
        m.auto = auto;
        m.candidate = candidate;
        m.id = self.put_score_model(&m)?;
        let rep = self.rescore(&m)?;
        Ok((m, rep))
    }

    /// Score every setting again with a model, in one transaction: new scores, statuses
    /// (decided ones kept), and Quick Start rows.
    pub fn rescore(&self, m: &ScoreModel) -> Result<RescoreReport, String> {
        let all = self.settings(&SettingsQuery {
            order: Some("id".into()),
            ..Default::default()
        })?;
        let now = now_utc();
        self.write(|tx| {
            let mut rep = RescoreReport {
                model_id: m.id,
                settings: all.len(),
                ..Default::default()
            };
            for s in &all {
                let f = features(&s.descriptors, s.setting.soup < 1.0);
                let score = m.score(&f);
                let status = if s.status.decided() {
                    s.status
                } else {
                    SettingStatus::for_score(m, score)
                };
                tx.prepare_cached(
                    "UPDATE settings SET score = ?2, qs_status = ?3, model_id = ?4, updated = ?5
                     WHERE id = ?1",
                )
                .map_err(e)?
                .execute(params![s.id, score, status.name(), m.id, now])
                .map_err(e)?;
                if status != s.status {
                    *rep.changed.entry(status.name()).or_default() += 1;
                }
                if status != SettingStatus::Stored || s.status != SettingStatus::Stored {
                    sync_entry(tx, s.id, &s.setting, &s.descriptors, Some(score), status)?;
                }
                *rep.statuses.entry(status.name()).or_default() += 1;
            }
            Ok(rep)
        })
    }

    /// Thin the Quick Start's search picks and the approval queue: per space (tiling,
    /// neighbourhood, and rule family), the undecided `auto` and `candidate` settings best score
    /// first; a setting whose novelty vector lies within `min_distance` of one already admitted
    /// in its space is skipped as a look-alike; the first `auto_per_space` distinct `auto`
    /// settings stay `auto`, the next distinct ones (auto or candidate) fill
    /// `candidate_per_space` candidates, and the rest become `stored` (kept, not deleted).
    /// Approved and rejected settings are never changed, and they count as admitted, so a
    /// look-alike of an approved setting is skipped too. With `dry_run`, nothing is written.
    /// A later rescore (a refit, new thresholds) applies the thresholds again; `jetcarp pipeline`
    /// thins after its refit.
    pub fn thin_quickstart(&self, o: &ThinOptions) -> Result<ThinReport, String> {
        use crate::stage1::score::{distance, novelty_vector};
        let rows = self.settings(&SettingsQuery {
            status: vec![
                SettingStatus::Auto,
                SettingStatus::Candidate,
                SettingStatus::Approved,
            ],
            order: Some("score".into()),
            ..Default::default()
        })?;
        let mut rep = ThinReport::default();
        type Space = (String, bool, String);
        let mut admitted: HashMap<Space, (Vec<[f64; 16]>, usize, usize)> = HashMap::new();
        // Approved settings first (they are the user's).
        for r in rows.iter().filter(|r| r.status == SettingStatus::Approved) {
            let k = (
                r.tiling_name.clone(),
                r.notation.ends_with('V'),
                r.family.clone(),
            );
            admitted
                .entry(k)
                .or_default()
                .0
                .push(novelty_vector(&r.descriptors));
        }
        let mut changes: Vec<(&SettingRow, SettingStatus)> = Vec::new();
        for r in rows.iter().filter(|r| r.status != SettingStatus::Approved) {
            *rep.before.entry(r.status.name()).or_default() += 1;
            let k = (
                r.tiling_name.clone(),
                r.notation.ends_with('V'),
                r.family.clone(),
            );
            let a = admitted.entry(k).or_default();
            let v = novelty_vector(&r.descriptors);
            let alike = a.0.iter().any(|x| distance(x, &v) < o.min_distance);
            let main = matches!(r.setting.tiling.family.as_str(), "square" | "hex");
            let (cap_auto, cap_cand) = if main {
                (o.auto_main, o.candidate_main)
            } else {
                (o.auto_per_space, o.candidate_per_space)
            };
            let status = if alike {
                rep.look_alikes += 1;
                SettingStatus::Stored
            } else if r.status == SettingStatus::Auto && a.1 < cap_auto {
                a.1 += 1;
                SettingStatus::Auto
            } else if a.2 < cap_cand {
                a.2 += 1;
                SettingStatus::Candidate
            } else {
                SettingStatus::Stored
            };
            if status != SettingStatus::Stored {
                a.0.push(v);
            }
            *rep.after.entry(status.name()).or_default() += 1;
            if status != r.status {
                changes.push((r, status));
            }
        }
        rep.spaces = admitted.len();
        rep.changed = changes.len();
        if o.dry_run {
            return Ok(rep);
        }
        let now = now_utc();
        self.write(|tx| {
            for (r, st) in &changes {
                tx.prepare_cached("UPDATE settings SET qs_status = ?2, updated = ?3 WHERE id = ?1")
                    .map_err(e)?
                    .execute(params![r.id, st.name(), now])
                    .map_err(e)?;
                sync_entry(tx, r.id, &r.setting, &r.descriptors, r.score, *st)?;
            }
            Ok(())
        })?;
        Ok(rep)
    }

}

/// `12`, `setting-12`, or `find:setting-12`.
pub fn parse_setting_id(s: &str) -> Result<i64, String> {
    let t = s.trim().trim_start_matches("find:");
    let t = t.strip_prefix("setting-").unwrap_or(t);
    t.parse()
        .map_err(|_| format!("'{s}' is not a setting id (12 or setting-12)"))
}

fn settings_where(q: &SettingsQuery) -> (String, Vec<Sql>) {
    let mut w: Vec<String> = Vec::new();
    let mut args: Vec<Sql> = Vec::new();
    let mut arg = |v: Sql| {
        args.push(v);
        format!("?{}", args.len())
    };
    if !q.status.is_empty() {
        let names: Vec<String> = q.status.iter().map(|s| format!("'{}'", s.name())).collect();
        w.push(format!("s.qs_status IN ({})", names.join(", ")));
    }
    if let Some(j) = q.job {
        w.push(format!("s.job_id = {}", arg(Sql::Integer(j))));
    }
    if let Some(t) = &q.tiling {
        let a = arg(Sql::Text(t.clone()));
        let b = arg(Sql::Text(format!("%\"family\":\"{t}\"%")));
        w.push(format!(
            "(s.tiling_name = {a} COLLATE NOCASE OR s.tiling LIKE {b})"
        ));
    }
    if let Some(r) = &q.rule {
        let t = r.trim();
        let (bare, edge) = match t.strip_suffix('V') {
            Some(b) => (b.to_string(), true),
            None => (t.to_string(), false),
        };
        let a = arg(Sql::Text(bare));
        if edge {
            w.push(format!("r.notation = {a} AND r.neighborhood = 'edge'"));
        } else {
            w.push(format!("r.notation = {a}"));
        }
    }
    if let Some(f) = &q.family {
        w.push(format!("r.family = {}", arg(Sql::Text(f.clone()))));
    }
    if let Some(b) = &q.behaviour {
        w.push(format!("s.behaviour = {}", arg(Sql::Text(b.clone()))));
    }
    if let Some(m) = q.min_score {
        w.push(format!("s.score >= {}", arg(Sql::Real(m))));
    }
    let filter = if w.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", w.join(" AND "))
    };
    (filter, args)
}
