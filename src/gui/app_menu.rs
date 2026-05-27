//
// app_menu.rs
// Copyright (C) 2026 b1ngggg
// Distributed under terms of the GPL-3.0-or-later license.
//

use gtk::{prelude::*, *};
use std::cell::RefCell;

#[derive(Clone, Copy)]
pub struct OverlayMenuState<'a> {
    pub overlay: &'a Overlay,
    pub layer_state: &'a RefCell<Option<Widget>>,
    pub card_state: &'a RefCell<Option<Widget>>,
}

#[derive(Clone, Copy)]
pub struct PointMenuPlacement<'a> {
    pub anchor: &'a Widget,
    pub width: i32,
    pub estimated_height: i32,
    pub x: f64,
    pub y: f64,
    pub extra_card_class: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub struct AnchorMenuPlacement<'a> {
    pub anchor: &'a Widget,
    pub width: i32,
    pub estimated_height: i32,
    pub above: bool,
    pub fallback_end_margin: i32,
}

#[derive(Clone, Copy)]
struct OverlayMenuPlacement<'a> {
    width: i32,
    x: i32,
    y: i32,
    extra_card_class: Option<&'a str>,
}

pub fn separator() -> Separator {
    let separator = Separator::new(Orientation::Horizontal);
    separator.add_css_class("app-menu-separator");
    separator
}

pub fn text_row(label: &str) -> Button {
    let button = Button::with_label(label);
    button.add_css_class("flat");
    button.add_css_class("app-menu-row");
    button
}

pub fn action_row<F: Fn() + 'static>(icon: &str, label: &str, action: F) -> Button {
    let button = Button::new();
    button.add_css_class("flat");
    button.add_css_class("app-menu-row");

    let box_ = Box::new(Orientation::Horizontal, 10);
    box_.set_hexpand(true);

    let image = Image::from_icon_name(icon);
    image.set_pixel_size(16);
    image.add_css_class("app-menu-row-icon");
    box_.append(&image);

    let label = Label::new(Some(label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    box_.append(&label);

    button.set_child(Some(&box_));
    button.connect_clicked(move |_| action());
    button
}

pub fn choice_row(label: &str, selected: bool) -> Button {
    icon_choice_row("", label, selected)
}

pub fn icon_choice_row(icon: &str, label: &str, selected: bool) -> Button {
    let button = Button::new();
    button.add_css_class("flat");
    button.add_css_class("app-menu-row");
    if selected {
        button.add_css_class("selected");
    }

    let box_ = Box::new(Orientation::Horizontal, 10);
    box_.set_hexpand(true);

    if icon.is_empty() {
        let spacer = Box::new(Orientation::Horizontal, 0);
        spacer.set_width_request(16);
        box_.append(&spacer);
    } else {
        let image = Image::from_icon_name(icon);
        image.set_pixel_size(16);
        image.add_css_class("app-menu-row-icon");
        box_.append(&image);
    }

    let label = Label::new(Some(label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    box_.append(&label);

    let check = Image::from_icon_name("object-select-symbolic");
    check.set_pixel_size(15);
    check.set_visible(selected);
    check.add_css_class("app-menu-check");
    box_.append(&check);

    button.set_child(Some(&box_));
    button
}

pub fn clear_overlay_menu(
    overlay: &Overlay,
    layer_state: &RefCell<Option<Widget>>,
    card_state: &RefCell<Option<Widget>>,
) {
    if let Some(layer) = layer_state.borrow_mut().take() {
        overlay.remove_overlay(&layer);
    }
    card_state.borrow_mut().take();
}

pub fn contains_widget(card_state: &RefCell<Option<Widget>>, widget: &Widget) -> bool {
    card_state
        .borrow()
        .as_ref()
        .map(|card| widget == card || widget.is_ancestor(card))
        .unwrap_or_default()
}

pub fn show_point_menu<F>(
    state: OverlayMenuState<'_>,
    placement: PointMenuPlacement<'_>,
    content: &impl IsA<Widget>,
    dismiss: F,
) where
    F: Fn() + 'static,
{
    let point = placement
        .anchor
        .compute_point(
            state.overlay,
            &gtk::graphene::Point::new(placement.x as f32, placement.y as f32),
        )
        .unwrap_or_else(|| gtk::graphene::Point::new(placement.x as f32, placement.y as f32));

    let overlay_width = state.overlay.allocated_width().max(placement.width + 24);
    let overlay_height = state
        .overlay
        .allocated_height()
        .max(placement.estimated_height + 24);
    let menu_x = (point.x() as i32).clamp(8, (overlay_width - placement.width - 8).max(8));
    let menu_y =
        (point.y() as i32).clamp(8, (overlay_height - placement.estimated_height - 8).max(8));

    show_overlay_menu(
        state,
        content,
        OverlayMenuPlacement {
            width: placement.width,
            x: menu_x,
            y: menu_y,
            extra_card_class: placement.extra_card_class,
        },
        dismiss,
    );
}

pub fn show_anchor_menu<F>(
    state: OverlayMenuState<'_>,
    placement: AnchorMenuPlacement<'_>,
    content: &impl IsA<Widget>,
    dismiss: F,
) where
    F: Fn() + 'static,
{
    let overlay_width = state
        .overlay
        .allocated_width()
        .max(placement.width + placement.fallback_end_margin + 12);
    let overlay_height = state
        .overlay
        .allocated_height()
        .max(placement.estimated_height + 24);
    let anchor_point = placement
        .anchor
        .compute_point(state.overlay, &gtk::graphene::Point::new(0.0, 0.0));
    let anchor_height = placement.anchor.allocated_height();
    let anchor_width = placement.anchor.allocated_width();
    let (mut x, mut y) = if let Some(point) = anchor_point {
        let x = point.x() as i32 + anchor_width - placement.width;
        let y = if placement.above {
            point.y() as i32 - placement.estimated_height - 8
        } else {
            point.y() as i32 + anchor_height + 8
        };
        (x, y)
    } else {
        let x = overlay_width - placement.width - placement.fallback_end_margin;
        let y = if placement.above {
            overlay_height - placement.estimated_height - 18
        } else {
            12
        };
        (x, y)
    };

    x = x.clamp(12, (overlay_width - placement.width - 12).max(12));
    y = y.clamp(
        12,
        (overlay_height - placement.estimated_height - 12).max(12),
    );

    show_overlay_menu(
        state,
        content,
        OverlayMenuPlacement {
            width: placement.width,
            x,
            y,
            extra_card_class: None,
        },
        dismiss,
    );
}

fn show_overlay_menu<F>(
    state: OverlayMenuState<'_>,
    content: &impl IsA<Widget>,
    placement: OverlayMenuPlacement<'_>,
    dismiss: F,
) where
    F: Fn() + 'static,
{
    let layer = gtk::Fixed::new();
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    layer.set_halign(Align::Fill);
    layer.set_valign(Align::Fill);
    layer.add_css_class("app-menu-layer");

    let card = Box::new(Orientation::Vertical, 0);
    card.set_width_request(placement.width);
    card.add_css_class("app-menu-card");
    if let Some(extra_class) = placement.extra_card_class {
        card.add_css_class(extra_class);
    }
    card.append(content);

    let card_widget = card.clone().upcast::<Widget>();
    layer.put(&card, f64::from(placement.x), f64::from(placement.y));
    state.overlay.add_overlay(&layer);
    state.overlay.set_measure_overlay(&layer, false);

    state
        .layer_state
        .replace(Some(layer.clone().upcast::<Widget>()));
    state.card_state.replace(Some(card_widget.clone()));

    let gesture = GestureClick::new();
    gesture.set_button(0);
    gesture.set_propagation_phase(PropagationPhase::Capture);
    gesture.connect_pressed(move |gesture, _, x, y| {
        let target = gesture
            .widget()
            .and_then(|widget| widget.pick(x, y, gtk::PickFlags::DEFAULT));
        let inside_card = target
            .as_ref()
            .map(|target| target == &card_widget || target.is_ancestor(&card_widget))
            .unwrap_or_default();
        if inside_card {
            return;
        }
        dismiss();
        gesture.set_state(EventSequenceState::Claimed);
    });
    layer.add_controller(gesture);
}
