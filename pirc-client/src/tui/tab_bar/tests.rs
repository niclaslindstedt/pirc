use super::*;

/// Helper: extract a row of characters from the buffer as a String.
fn row_text(buf: &Buffer, row: u16, start: u16, width: u16) -> String {
    (start..start + width)
        .map(|col| buf.get(col, row).ch)
        .collect()
}

/// Helper: extract the style at a specific column.
fn cell_style(buf: &Buffer, col: u16, row: u16) -> Style {
    buf.get(col, row).style
}

// --- TabInfo ---

#[test]
fn tab_info_display_text_no_unread() {
    let tab = TabInfo {
        label: "#general".into(),
        is_active: false,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.display_text(), "#general");
}

#[test]
fn tab_info_display_text_with_unread() {
    let tab = TabInfo {
        label: "#general".into(),
        is_active: false,
        unread_count: 5,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.display_text(), "#general [5]");
}

#[test]
fn tab_info_style_active() {
    let tab = TabInfo {
        label: "test".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.style(), STYLE_TAB_ACTIVE);
}

#[test]
fn tab_info_style_active_overrides_unread() {
    let tab = TabInfo {
        label: "test".into(),
        is_active: true,
        unread_count: 3,
        has_activity: true,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.style(), STYLE_TAB_ACTIVE);
}

#[test]
fn tab_info_style_unread() {
    let tab = TabInfo {
        label: "test".into(),
        is_active: false,
        unread_count: 1,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.style(), STYLE_TAB_UNREAD);
}

#[test]
fn tab_info_style_activity() {
    let tab = TabInfo {
        label: "test".into(),
        is_active: false,
        unread_count: 0,
        has_activity: true,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.style(), STYLE_TAB_ACTIVITY);
}

#[test]
fn tab_info_style_normal() {
    let tab = TabInfo {
        label: "test".into(),
        is_active: false,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.style(), STYLE_TAB_NORMAL);
}

// --- total_tabs_width ---

#[test]
fn total_width_empty() {
    assert_eq!(total_tabs_width(&[]), 0);
}

#[test]
fn total_width_single_tab() {
    let tabs = [TabInfo {
        label: "Status".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    // "Status" = 6 chars, no separators
    assert_eq!(total_tabs_width(&tabs), 6);
}

#[test]
fn total_width_two_tabs() {
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#a".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    // "Status" + " | " + "#a" = 6 + 3 + 2 = 11
    assert_eq!(total_tabs_width(&tabs), 11);
}

#[test]
fn total_width_with_unread() {
    let tabs = [TabInfo {
        label: "#a".into(),
        is_active: false,
        unread_count: 3,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    // "#a [3]" = 6
    assert_eq!(total_tabs_width(&tabs), 6);
}

// --- compute_visible_range ---

#[test]
fn visible_range_all_fit() {
    let tabs = vec![
        TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#a".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    let (start, end) = compute_visible_range(&tabs, 80, 0);
    assert_eq!((start, end), (0, 2));
}

#[test]
fn visible_range_empty() {
    let (start, end) = compute_visible_range(&[], 80, 0);
    assert_eq!((start, end), (0, 0));
}

#[test]
fn visible_range_overflow_keeps_active() {
    // Many tabs that won't fit in 20 columns
    let tabs: Vec<TabInfo> = (0..10)
        .map(|i| TabInfo {
            label: format!("#{}", i),
            is_active: i == 5,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    let (start, end) = compute_visible_range(&tabs, 20, 5);
    assert!(start <= 5);
    assert!(end > 5);
}

#[test]
fn visible_range_active_at_start() {
    let tabs: Vec<TabInfo> = (0..10)
        .map(|i| TabInfo {
            label: format!("chan{}", i),
            is_active: i == 0,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    let (start, _end) = compute_visible_range(&tabs, 25, 0);
    assert_eq!(start, 0);
}

#[test]
fn visible_range_active_at_end() {
    let tabs: Vec<TabInfo> = (0..10)
        .map(|i| TabInfo {
            label: format!("chan{}", i),
            is_active: i == 9,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    let (_start, end) = compute_visible_range(&tabs, 25, 9);
    assert_eq!(end, 10);
}

// --- render_tab_bar ---

#[test]
fn render_empty_tabs() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    render_tab_bar(&mut buf, &region, &[]);
    // Entire row should be spaces
    let text = row_text(&buf, 0, 0, 80);
    assert_eq!(text.trim(), "");
}

#[test]
fn render_single_active_tab() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [TabInfo {
        label: "Status".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 6);
    assert_eq!(text, "Status");
    assert_eq!(cell_style(&buf, 0, 0), STYLE_TAB_ACTIVE);
    assert_eq!(cell_style(&buf, 5, 0), STYLE_TAB_ACTIVE);
    // Rest should be cleared to normal
    assert_eq!(cell_style(&buf, 6, 0), STYLE_TAB_NORMAL);
}

#[test]
fn render_two_tabs_with_separator() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#rust".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    // "Status | #rust"
    let text = row_text(&buf, 0, 0, 14);
    assert_eq!(text, "Status | #rust");

    // Separator style
    assert_eq!(cell_style(&buf, 7, 0), STYLE_TAB_SEPARATOR); // "|"

    // Second tab uses normal style
    assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_NORMAL); // "#"
}

#[test]
fn render_tab_with_unread() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#chat".into(),
            is_active: false,
            unread_count: 3,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    // "Status | #chat [3]"
    let text = row_text(&buf, 0, 0, 18);
    assert_eq!(text, "Status | #chat [3]");

    // Unread tab should have STYLE_TAB_UNREAD
    assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_UNREAD);
}

#[test]
fn render_tab_with_activity() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#chat".into(),
            is_active: false,
            unread_count: 0,
            has_activity: true,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_ACTIVITY);
}

#[test]
fn render_with_region_offset() {
    let mut buf = Buffer::new(80, 5);
    let region = Rect::new(5, 2, 30, 1);
    let tabs = [TabInfo {
        label: "Status".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    render_tab_bar(&mut buf, &region, &tabs);

    // Tab text should start at col 5, row 2
    assert_eq!(buf.get(5, 2).ch, 'S');
    assert_eq!(buf.get(10, 2).ch, 's'); // last char of "Status"
    assert_eq!(buf.get(11, 2).ch, ' '); // after "Status"
                                        // Nothing should be written to row 0
    assert_eq!(buf.get(5, 0).ch, ' ');
    assert_eq!(cell_style(&buf, 5, 0), Style::new());
}

#[test]
fn render_overflow_shows_indicators() {
    // 20 cols, try to render many tabs
    let mut buf = Buffer::new(20, 1);
    let region = Rect::new(0, 0, 20, 1);
    let tabs: Vec<TabInfo> = (0..6)
        .map(|i| TabInfo {
            label: format!("ch{}", i),
            is_active: i == 3,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    render_tab_bar(&mut buf, &region, &tabs);

    let text: String = row_text(&buf, 0, 0, 20);
    // Active tab (ch3) should be visible
    assert!(
        text.contains("ch3"),
        "Active tab should be visible: '{}'",
        text
    );
    // Should have overflow indicator(s)
    let has_left = text.starts_with("< ");
    let has_right = text.trim_end().ends_with(">");
    assert!(
        has_left || has_right,
        "Should have overflow indicators: '{}'",
        text
    );
}

#[test]
fn render_overflow_active_at_beginning() {
    let mut buf = Buffer::new(20, 1);
    let region = Rect::new(0, 0, 20, 1);
    let tabs: Vec<TabInfo> = (0..10)
        .map(|i| TabInfo {
            label: format!("chan{}", i),
            is_active: i == 0,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 20);
    // Active tab chan0 should be at the beginning, no left indicator
    assert!(
        text.starts_with("chan0"),
        "Should start with active tab: '{}'",
        text
    );
    assert!(
        text.trim_end().ends_with(">"),
        "Should have right overflow: '{}'",
        text
    );
}

#[test]
fn render_overflow_active_at_end() {
    let mut buf = Buffer::new(20, 1);
    let region = Rect::new(0, 0, 20, 1);
    let tabs: Vec<TabInfo> = (0..10)
        .map(|i| TabInfo {
            label: format!("chan{}", i),
            is_active: i == 9,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        })
        .collect();
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 20);
    assert!(
        text.contains("chan9"),
        "Active tab should be visible: '{}'",
        text
    );
    assert!(
        text.starts_with("< "),
        "Should have left overflow: '{}'",
        text
    );
}

#[test]
fn render_clears_entire_region() {
    let mut buf = Buffer::new(80, 1);
    // Pre-fill with content
    let fill_style = Style::new().fg(Color::Red);
    buf.write_str(0, 0, "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", fill_style);
    buf.clear_dirty();

    let region = Rect::new(0, 0, 80, 1);
    let tabs = [TabInfo {
        label: "A".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    render_tab_bar(&mut buf, &region, &tabs);

    // First char is 'A'
    assert_eq!(buf.get(0, 0).ch, 'A');
    // The old 'X' at col 1 should now be space
    assert_eq!(buf.get(1, 0).ch, ' ');
    assert_eq!(cell_style(&buf, 1, 0), STYLE_TAB_NORMAL);
}

#[test]
fn render_zero_width_region() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 0, 1);
    // Should not panic
    render_tab_bar(
        &mut buf,
        &region,
        &[TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        }],
    );
}

#[test]
fn render_zero_height_region() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 0);
    // Should not panic
    render_tab_bar(
        &mut buf,
        &region,
        &[TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        }],
    );
}

#[test]
fn render_narrow_truncates_gracefully() {
    // Region only 3 columns wide
    let mut buf = Buffer::new(3, 1);
    let region = Rect::new(0, 0, 3, 1);
    let tabs = [TabInfo {
        label: "Status".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    }];
    render_tab_bar(&mut buf, &region, &tabs);

    // Should show "Sta" truncated
    let text = row_text(&buf, 0, 0, 3);
    assert_eq!(text, "Sta");
}

#[test]
fn render_three_tabs_all_fit() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#general".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#rust".into(),
            is_active: false,
            unread_count: 2,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    // "Status | #general | #rust [2]"
    let expected = "Status | #general | #rust [2]";
    let text = row_text(&buf, 0, 0, expected.len() as u16);
    assert_eq!(text, expected);
}

#[test]
fn render_no_active_tab_defaults_to_first() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "#a".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    // Both tabs should be rendered (no overflow for 80 cols)
    let text = row_text(&buf, 0, 0, 11);
    assert_eq!(text, "Status | #a");
}

#[test]
fn render_many_tabs_exact_fit() {
    // Calculate exact width needed for 3 tabs
    // "ab | cd | ef" = 2 + 3 + 2 + 3 + 2 = 12
    let mut buf = Buffer::new(12, 1);
    let region = Rect::new(0, 0, 12, 1);
    let tabs = [
        TabInfo {
            label: "ab".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "cd".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "ef".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 12);
    assert_eq!(text, "ab | cd | ef");
}

#[test]
fn style_constants_are_correct() {
    assert!(STYLE_TAB_ACTIVE.bold);
    assert!(STYLE_TAB_ACTIVE.reverse);
    assert!(STYLE_TAB_UNREAD.bold);
    assert_eq!(STYLE_TAB_UNREAD.fg, Some(Color::Yellow));
    assert!(STYLE_TAB_ACTIVITY.bold);
    assert_eq!(STYLE_TAB_ACTIVITY.fg, Some(Color::Cyan));
    assert!(STYLE_TAB_NORMAL.is_empty());
    assert_eq!(STYLE_TAB_SEPARATOR.fg, Some(Color::BrightBlack));
}

// --- Encryption status in display text ---

#[test]
fn display_text_e2e_active() {
    let tab = TabInfo {
        label: "bob".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::Active,
    };
    assert_eq!(tab.display_text(), "[E2E]bob");
}

#[test]
fn display_text_e2e_establishing() {
    let tab = TabInfo {
        label: "bob".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::Establishing,
    };
    assert_eq!(tab.display_text(), "[...]bob");
}

#[test]
fn display_text_e2e_none() {
    let tab = TabInfo {
        label: "bob".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    assert_eq!(tab.display_text(), "bob");
}

#[test]
fn display_text_e2e_with_unread() {
    let tab = TabInfo {
        label: "bob".into(),
        is_active: false,
        unread_count: 3,
        has_activity: false,
        encryption_status: EncryptionStatus::Active,
    };
    assert_eq!(tab.display_text(), "[E2E]bob [3]");
}

#[test]
fn render_tab_with_e2e_indicator() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "bob".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::Active,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 18);
    assert_eq!(text, "Status | [E2E]bob ");
}

#[test]
fn render_tab_with_establishing_indicator() {
    let mut buf = Buffer::new(80, 1);
    let region = Rect::new(0, 0, 80, 1);
    let tabs = [
        TabInfo {
            label: "Status".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::None,
        },
        TabInfo {
            label: "bob".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
            encryption_status: EncryptionStatus::Establishing,
        },
    ];
    render_tab_bar(&mut buf, &region, &tabs);

    let text = row_text(&buf, 0, 0, 18);
    assert_eq!(text, "Status | [...]bob ");
}

#[test]
fn channel_tab_no_encryption_indicator() {
    let tab = TabInfo {
        label: "#general".into(),
        is_active: true,
        unread_count: 0,
        has_activity: false,
        encryption_status: EncryptionStatus::None,
    };
    // Channels should never have encryption indicators
    assert_eq!(tab.display_text(), "#general");
}
