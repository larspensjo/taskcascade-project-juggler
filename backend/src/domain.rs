/// Return a copy of an ordered ID list after moving `id` beside `target`.
/// Keeping this independent from storage makes the ordering invariant testable.
pub fn relocate(
    ids: &[String],
    id: &str,
    target: Option<&str>,
    after: bool,
) -> Option<Vec<String>> {
    let mut ordered = ids.to_vec();
    let old_index = ordered.iter().position(|candidate| candidate == id)?;
    let moved = ordered.remove(old_index);
    let insert_at = match target {
        Some(target_id) => {
            let target_index = ordered
                .iter()
                .position(|candidate| candidate == target_id)?;
            target_index + usize::from(after)
        }
        None if after => ordered.len(),
        None => 0,
    };
    ordered.insert(insert_at, moved);
    Some(ordered)
}

#[cfg(test)]
mod tests {
    use super::relocate;

    #[test]
    fn relocates_after_target_without_losing_tasks() {
        let ids = ["a", "b", "c"].map(str::to_owned);
        assert_eq!(
            relocate(&ids, "a", Some("c"), true),
            Some(["b", "c", "a"].map(str::to_owned).to_vec())
        );
    }
}
