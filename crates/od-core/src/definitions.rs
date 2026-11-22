//! Rules for editing OD definitions and display-field grids.
//!
//! The database layer writes exactly the OD columns. These helpers keep the
//! grid invariants OD enforces in `FormDefinitions`, `FormDisplayFields` and
//! `DefL`: stable item order, non-empty names and unique display labels.

use std::collections::HashSet;

use od_domain::definitions::*;

/// Definitions for one category (or all categories), sorted the way OD's
/// `FormDefinitions` grid shows them. Hidden rows are included only on request.
pub fn sorted_definitions(
    definitions: &[Definition],
    category: Option<DefinitionCategory>,
    include_hidden: bool,
) -> Vec<Definition> {
    let mut rows: Vec<Definition> = definitions
        .iter()
        .filter(|d| category.map_or(true, |c| d.category == c))
        .filter(|d| include_hidden || !d.is_hidden)
        .cloned()
        .collect();
    rows.sort_by(|a, b| a.item_order.cmp(&b.item_order).then(a.def_num.cmp(&b.def_num)));
    rows
}

/// Next `ItemOrder` for a new OD definition in a category.
pub fn next_definition_order(definitions: &[Definition], category: DefinitionCategory) -> i32 {
    definitions
        .iter()
        .filter(|d| d.category == category)
        .map(|d| d.item_order)
        .max()
        .unwrap_or(-1)
        + 1
}

/// Validate a single definition before it reaches the store.
pub fn validate_definition(definition: &Definition) -> Result<(), String> {
    if definition.item_name.trim().is_empty() {
        return Err("Definition name is required.".into());
    }
    if definition.item_order < 0 {
        return Err("Definition order cannot be negative.".into());
    }
    Ok(())
}

/// Re-number a category after a delete, matching `FormDefinitions_Closing`.
pub fn compact_definition_orders(definitions: &mut [Definition], category: DefinitionCategory) {
    let mut rows: Vec<&mut Definition> = definitions
        .iter_mut()
        .filter(|d| d.category == category)
        .collect();
    rows.sort_by_key(|d| (d.item_order, d.def_num));
    for (order, definition) in rows.into_iter().enumerate() {
        definition.item_order = order as i32;
    }
}

/// Display-field rows for a category in OD grid order.
pub fn display_fields_for(fields: &[DisplayField], category: DisplayFieldCategory) -> Vec<DisplayField> {
    let mut rows: Vec<DisplayField> = fields
        .iter()
        .filter(|f| f.category == category)
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        a.item_order
            .cmp(&b.item_order)
            .then(a.display_field_num.cmp(&b.display_field_num))
    });
    rows
}

/// Validate the grid rules OD applies when `FormDisplayFields` saves a
/// category. Both the internal names and the translated descriptions must be
/// unique, and `ItemOrder` must not contain duplicates.
pub fn validate_display_fields(fields: &[DisplayField]) -> Result<(), String> {
    let mut orders = HashSet::new();
    let mut descriptions = HashSet::new();
    for field in fields {
        if field.internal_name.trim().is_empty() {
            return Err("Every display field needs an internal name.".into());
        }
        if field.column_width < 0 {
            return Err(format!("Column width for {} cannot be negative.", field.internal_name));
        }
        if !orders.insert(field.item_order) {
            return Err(format!("Duplicate display-field order {}.", field.item_order));
        }
        // OD's `ContainsDuplicateDescriptions()` ignores blank descriptions,
        // then rejects a description repeated anywhere or equal to any
        // InternalName. `DescriptionOverride` is only used by Ortho grids.
        let description = field.description.trim();
        if !description.is_empty() {
            if fields
                .iter()
                .any(|other| other.internal_name == description)
                || !descriptions.insert(description.to_lowercase())
            {
                return Err(format!(
                    "Display Fields cannot have duplicate descriptions or descriptions matching FieldName: {description}"
                ));
            }
        }
    }
    Ok(())
}

/// Re-number display fields in their current order, as OD does when saving a
/// category.
pub fn compact_display_field_orders(fields: &mut [DisplayField], category: DisplayFieldCategory) {
    let mut rows: Vec<&mut DisplayField> = fields
        .iter_mut()
        .filter(|f| f.category == category)
        .collect();
    rows.sort_by_key(|f| (f.item_order, f.display_field_num));
    for (order, field) in rows.into_iter().enumerate() {
        field.item_order = order as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(num: u32, category: DefinitionCategory, order: i32, hidden: bool) -> Definition {
        Definition {
            def_num: num,
            category,
            item_order: order,
            item_name: format!("Item {num}"),
            is_hidden: hidden,
            ..Definition::default()
        }
    }

    fn field(name: &str, label: &str, order: i32) -> DisplayField {
        DisplayField {
            internal_name: name.into(),
            description: label.into(),
            item_order: order,
            column_width: 80,
            category: DisplayFieldCategory::PATIENT_SELECT,
            ..DisplayField::default()
        }
    }

    #[test]
    fn definitions_sort_by_order_and_hide_when_asked() {
        let rows = vec![
            def(2, DefinitionCategory::PAYMENT_TYPES, 1, false),
            def(1, DefinitionCategory::PAYMENT_TYPES, 0, false),
            def(3, DefinitionCategory::PAYMENT_TYPES, 2, true),
            def(4, DefinitionCategory::ADJ_TYPES, 0, false),
        ];
        let visible = sorted_definitions(&rows, Some(DefinitionCategory::PAYMENT_TYPES), false);
        assert_eq!(visible.iter().map(|d| d.def_num).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(next_definition_order(&rows, DefinitionCategory::PAYMENT_TYPES), 3);
        assert_eq!(next_definition_order(&rows, DefinitionCategory::BILLING_TYPES), 0);
    }

    #[test]
    fn definition_validation_and_compaction() {
        assert!(validate_definition(&def(1, DefinitionCategory::ADJ_TYPES, 0, false)).is_ok());
        let mut bad = def(1, DefinitionCategory::ADJ_TYPES, 0, false);
        bad.item_name = "  ".into();
        assert!(validate_definition(&bad).is_err());
        bad.item_name = "Cash".into();
        bad.item_order = -1;
        assert!(validate_definition(&bad).is_err());

        let mut rows = vec![
            def(1, DefinitionCategory::ADJ_TYPES, 4, false),
            def(2, DefinitionCategory::ADJ_TYPES, 9, false),
        ];
        compact_definition_orders(&mut rows, DefinitionCategory::ADJ_TYPES);
        assert_eq!(rows[0].item_order, 0);
        assert_eq!(rows[1].item_order, 1);
    }

    #[test]
    fn display_field_validation_matches_od_rules() {
        let rows = vec![field("Hm Phone", "Phone", 0), field("Wk Phone", "Work", 1)];
        assert!(validate_display_fields(&rows).is_ok());

        let duplicate = vec![field("Hm Phone", "Phone", 0), field("Wk Phone", "Phone", 1)];
        assert!(validate_display_fields(&duplicate).is_err());

        let matching_name = vec![field("Phone", "Phone", 0)];
        assert!(validate_display_fields(&matching_name).is_err());

        let duplicate_order = vec![field("Hm Phone", "Home", 0), field("Wk Phone", "Work", 0)];
        assert!(validate_display_fields(&duplicate_order).is_err());
    }

    #[test]
    fn display_fields_sort_by_item_order() {
        let mut rows = vec![field("B", "Second", 1), field("A", "First", 0)];
        compact_display_field_orders(&mut rows, DisplayFieldCategory::PATIENT_SELECT);
        let sorted = display_fields_for(&rows, DisplayFieldCategory::PATIENT_SELECT);
        assert_eq!(sorted[0].internal_name, "A");
        assert_eq!(sorted[1].internal_name, "B");
    }
}
