use super::*;
use crate::history::rewrites::RewriteVariant;

impl HistoryStore {
    pub(crate) fn rewrite_variants(&self, id: &str) -> AppResult<Vec<RewriteVariant>> {
        let mut query = self.connection.prepare(
            "SELECT payload_json FROM history_rewrites WHERE entry_id=?1 ORDER BY created_at DESC,rowid DESC"
        ).map_err(database_error)?;
        let rows = query
            .query_map([id], |row| row.get::<_, String>(0))
            .map_err(database_error)?;
        rows.map(|row| Ok(serde_json::from_str(&row.map_err(database_error)?)?))
            .collect()
    }

    pub(crate) fn save_rewrite(
        &mut self,
        variant: &RewriteVariant,
        current: impl Fn() -> bool,
    ) -> AppResult<RewriteVariant> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        transaction.execute(
            "INSERT INTO history_rewrites(id,entry_id,source,created_at,payload_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO NOTHING",
            params![variant.id, variant.entry_id, if variant.source == crate::history::rewrites::RewriteSource::Original { "original" } else { "result" }, variant.created_at.timestamp_micros(), serde_json::to_string(variant)?],
        ).map_err(database_error)?;
        if !current() {
            return Err(crate::history::rewrites::unavailable());
        }
        transaction.commit().map_err(database_error)?;
        self.revision = self.revision.wrapping_add(1);
        self.rewrite_variants(&variant.entry_id)?
            .into_iter()
            .find(|value| value.id == variant.id)
            .ok_or_else(crate::history::rewrites::unavailable)
    }

    pub(crate) fn delete_rewrite(&mut self, entry_id: &str, id: &str) -> AppResult<()> {
        let changed = self
            .connection
            .execute(
                "DELETE FROM history_rewrites WHERE id=?1 AND entry_id=?2",
                params![id, entry_id],
            )
            .map_err(database_error)?;
        if changed > 0 {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
}
