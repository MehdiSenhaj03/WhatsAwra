//! A whole row as one flat button (Awra's `row-action`): it lights under the
//! pointer, presses, and stays lit while it is the chosen one.

/// Wraps `content` in a row button that calls `on_click`, checked while
/// `chosen`.
pub fn button(
    content: &impl awra::IsWidget,
    chosen: bool,
    on_click: impl Fn() + 'static,
) -> awra::ToggleButton {
    let row = awra::ToggleButton::new();
    awra::add_style_role(&row, "row-action");
    row.set_child(Some(content));
    row.set_active(chosen);
    row.connect_clicked(move |row| {
        // What is chosen is the app's to say: the next turn shows it.
        row.set_active(chosen);
        on_click();
    });
    row
}

/// Brings a list store from `old` to `new` by keys with one splice: the
/// rows both share at the start and at the end stay where they are (and
/// keep their scroll place), only the run between them is replaced; then
/// `rebind` is called for each kept row whose content changed.
pub fn update<T: PartialEq>(
    store: &awra::ListStore,
    old: &[T],
    new: &[T],
    key: impl Fn(&T) -> &str,
    rebind: impl Fn(u32),
) {
    let (prefix, suffix) = kept(
        &old.iter().map(&key).collect::<Vec<_>>(),
        &new.iter().map(&key).collect::<Vec<_>>(),
    );
    let removed = old.len() - prefix - suffix;
    let added = &new[prefix..new.len() - suffix];
    if removed > 0 || !added.is_empty() {
        let objects: Vec<awra::Object> = added
            .iter()
            .map(|item| awra::Object::new(awra::StringObject(key(item).to_owned())))
            .collect();
        store.splice(prefix as u32, removed as u32, &objects);
    }
    for (position, (before, after)) in old[..prefix].iter().zip(&new[..prefix]).enumerate() {
        if before != after {
            rebind(position as u32);
        }
    }
    let new_start = new.len() - suffix;
    for (offset, (before, after)) in old[old.len() - suffix..]
        .iter()
        .zip(&new[new_start..])
        .enumerate()
    {
        if before != after {
            rebind((new_start + offset) as u32);
        }
    }
}

/// How many keys `old` and `new` share at their start and, after those, at
/// their end.
fn kept(old: &[&str], new: &[&str]) -> (usize, usize) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let most = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(most)
        .take_while(|(a, b)| a == b)
        .count();
    (prefix, suffix)
}

#[cfg(test)]
mod tests {
    use super::kept;

    #[test]
    fn a_chat_moving_to_the_top_replaces_only_the_run_it_crossed() {
        assert_eq!(kept(&["a", "b", "c", "d"], &["c", "a", "b", "d"]), (0, 1));
    }

    #[test]
    fn messages_arriving_keep_every_row_already_there() {
        assert_eq!(kept(&["a", "b"], &["a", "b", "c"]), (2, 0));
        assert_eq!(kept(&["b", "c"], &["a", "b", "c"]), (0, 2));
    }

    #[test]
    fn a_repeated_key_is_not_counted_twice() {
        assert_eq!(kept(&["a", "a"], &["a"]), (1, 0));
    }
}
