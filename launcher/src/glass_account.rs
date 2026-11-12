//! Settings' Account card in the glass look: who plays now (a big head, the name, how) and the
//! accounts signed in here, in a plate of their own; beside it, the card that signs another
//! account in — ely.by's, this fork's way of playing, or Microsoft's when the build carries a
//! client id (`HORIZON_MSA_CLIENT_ID`).
//!
//! Not in the mockup, kept in its patterns: the accounts' Play as, Refresh and Sign out, the
//! sign-in's code, progress and problems.

use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use horizon_theme::glass::{ink, radius};

/// ely.by's green, from the site's own stylesheet. The site ships no logo file to embed (its
/// wordmark is styled text), so the mark here is a white E on that green, rounded like the plates.
const ELYBY_GREEN: u32 = 0x007e59;

/// ely.by's mark at `size` pixels square, where `microsoft::logo` draws Microsoft's squares.
fn elyby_mark(size: f32) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size * 0.3))
        .bg(gpui::rgb(ELYBY_GREEN))
        .font_family(horizon_theme::glass::DISPLAY_FONT)
        .font_weight(FontWeight::BOLD)
        .text_size(px(size * 0.58))
        .line_height(px(size * 0.58))
        .text_color(gpui::white())
        .child("E")
}

impl Launcher {
    pub(super) fn glass_account(&mut self, cx: &mut Context<Self>) -> Div {
        let wide = self.window_width >= crate::glass_home::WIDE;
        let left = self.glass_who_plays(cx);
        let right = self.glass_sign_in_card(cx);
        // Two plates of their own: the player and their accounts on the left, where the rows
        // take the width; what signs another in on the right. One under the other on a narrow
        // window. Each keeps its own edges, so neither floats in the section's card the way bare
        // text did.
        div()
            .flex()
            .when(wide, |d| d.flex_row())
            .when(!wide, |d| d.flex_col())
            .gap(px(if wide { 8. } else { 16. }))
            .child(
                left.when(wide, |d| d.flex_1().min_w(px(280.)))
                    .when(!wide, |d| d.flex_none().w_full())
                    .p(px(24.))
                    .rounded(px(radius::LARGE))
                    .bg(white(0.045)),
            )
            .child(
                right
                    .flex_none()
                    .when(wide, |d| d.w(px(400.)))
                    .when(!wide, |d| d.w_full()),
            )
    }

    /// The player now, and every account signed in here: the head and the name at the top, then a
    /// row per account with what it can do. The rows take the plate's width, so the page's left
    /// side is the accounts rather than empty glass.
    fn glass_who_plays(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let (uuid, name, how) = match self.accounts.selected() {
            Some(a) => {
                let how = match a.provider {
                    account::Provider::ElyBy => {
                        (Tone::Live, tr("ely.by account · online servers too"))
                    }
                    account::Provider::Microsoft => {
                        (Tone::Live, tr("Microsoft account · online servers too"))
                    }
                };
                (Some(a.uuid.clone()), a.name.clone(), how)
            }
            None => (
                None,
                tr("No account").to_string(),
                (Tone::Off, tr("Sign in to play")),
            ),
        };
        let head = div()
            .flex_none()
            .size(px(80.))
            .rounded(px(20.))
            .overflow_hidden()
            .child(
                self.profile_heads
                    .draw(uuid.as_deref(), 80., self.scale, &t),
            );
        let hero = div().flex().items_center().gap(px(20.)).child(head).child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(10.))
                .child(
                    div()
                        .font_family(horizon_theme::glass::DISPLAY_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(32.))
                        .line_height(px(32.))
                        .text_color(white(ink::FULL))
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(ui::dot(how.0).mt(px(7.)))
                        .child(
                            ui::text(14., FontWeight::NORMAL, 0.65)
                                .flex_1()
                                .min_w_0()
                                .line_height(px(21.))
                                .child(how.1),
                        ),
                ),
        );
        let rows = if self.accounts.accounts.is_empty() {
            None
        } else {
            Some(ui::flat_rows(|| self.microsoft_rows(cx)).0)
        };
        div()
            .flex()
            .flex_col()
            .child(hero)
            // The rows keep the plate's own 24 px edge: their 16 px inset and a pull of 16 line
            // the account's name up with the player's.
            .children(rows.map(|rows| rows.mx(px(-16.)).mt(px(20.)).mb(px(-8.))))
    }

    /// Signing an account in: the mark, what the card is for, the button that starts it and where
    /// it happens.
    fn glass_sign_in_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let has_accounts = !self.accounts.accounts.is_empty();
        let (title, detail, action) = self.sign_in_parts(cx);
        // This fork signs in with ely.by; Microsoft's own flow stays for a build that carries a
        // client id.
        let microsoft = account::client_id().is_some();
        let idle = self.sign_in_state.is_none()
            || matches!(self.sign_in_state, Some(SignInState::Done(_)));
        let first = !has_accounts && self.sign_in_target.is_none();
        let heading = if first {
            if microsoft {
                tr("Play online with your Microsoft account").to_string()
            } else {
                tr("Play online with your ely.by account").to_string()
            }
        } else {
            title
        };
        let note: AnyElement = if first && microsoft && self.sign_in_state.is_none() {
            div()
                .child(tr(
                    "Sign in with the account that owns Minecraft: Java Edition. Your name and \
                     skin come along to online servers, like in other launchers.",
                ))
                .into_any_element()
        } else {
            detail.into_any_element()
        };
        // Adding an account is the mockup's white button; signing one in again, the code and the
        // waiting keep their own buttons.
        let button: Option<AnyElement> = if idle && self.sign_in_target.is_none() {
            let (mark, label) = if microsoft {
                (microsoft::logo(18.), tr("Sign in with Microsoft"))
            } else {
                (elyby_mark(18.), tr("Sign in with ely.by"))
            };
            let plate = keyboard(
                div()
                    .id("glass-sign-in")
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .h(px(48.))
                    .w_full()
                    .mt(px(24.))
                    .rounded(px(radius::ROW))
                    .bg(gpui::white())
                    // On a light plate, the white button keeps an edge.
                    .when(ui::light(), |d| d.border_1().border_color(ui::white(0.15)))
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.9))
                    .font_family(horizon_theme::glass::FONT)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(0x100a1f))
                    .child(mark)
                    .child(label),
                &t,
            );
            let plate = if microsoft {
                plate.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.start_sign_in();
                    cx.notify();
                }))
            } else {
                plate.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.begin_elyby_sign_in();
                    cx.notify();
                }))
            };
            Some(plate.into_any_element())
        } else {
            action.map(|a| div().mt(px(20.)).child(a).into_any_element())
        };
        let where_: &str = if microsoft {
            tr("Microsoft's own page opens in your browser. Horizon never sees your password.")
        } else {
            tr("ely.by's own page opens in your browser. ElyHorizon never sees your password.")
        };
        div()
            .flex()
            .flex_col()
            .rounded(px(radius::LARGE))
            .bg(white(0.045))
            .p(px(24.))
            .child(if microsoft {
                microsoft::logo(36.)
            } else {
                elyby_mark(36.)
            })
            .child(
                div()
                    .mt(px(20.))
                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(22.))
                    .line_height(px(27.5))
                    .text_color(white(ink::FULL))
                    .child(heading),
            )
            .child(
                ui::text(14., FontWeight::NORMAL, 0.65)
                    .mt(px(8.))
                    .line_height(px(21.7))
                    .child(note),
            )
            .children(button)
            .child(
                ui::text(12., FontWeight::NORMAL, 0.45)
                    .mt(px(12.))
                    .line_height(px(18.))
                    .child(where_),
            )
    }
}
