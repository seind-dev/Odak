//! Groups: create groups, add members by Discord username, remove members, leave or delete a group.
//! These changes need the server, so they run online (see `account::online`) and then sync.

use crate::account;
use crate::model::{Group, Profile};
use crate::state::{AppState, Auth};
use crate::supabase::{self, Error};
use crate::sync;
use crate::theme::{self, Colors};
use crate::ui::icons::{self, icon};
use crate::ui::text_input::{TextEvent, TextInput};
use crate::ui::widgets::{button, chip, page_title, primary_button, user_avatar, user_name};
use gpui::{App, Context, Entity, FontWeight, IntoElement, Render, Subscription, WeakEntity, Window, div, prelude::*, px, white};
use uuid::Uuid;

/// Destructive actions wait for a second click.
#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    DeleteGroup(Uuid),
    Leave(Uuid),
    Remove(Uuid),
}

pub struct GroupsPage {
    selected: Option<Uuid>,
    new_group: Entity<TextInput>,
    new_member: Entity<TextInput>,
    confirm: Option<Confirm>,
    busy: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl GroupsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let new_group = cx.new(|cx| TextInput::new("Yeni grup adı", false, cx));
        let new_member = cx.new(|cx| TextInput::new("Discord kullanıcı adı", false, cx));
        let state = AppState::global(cx);
        let subscriptions = vec![
            cx.subscribe(&new_group, |this, _, event: &TextEvent, cx| {
                if let TextEvent::Submit = event {
                    this.create_group(cx);
                }
            }),
            cx.subscribe(&new_member, |this, _, event: &TextEvent, cx| {
                if let TextEvent::Submit = event {
                    this.add_member(cx);
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        GroupsPage {
            selected: None,
            new_group,
            new_member,
            confirm: None,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Runs an online request; on success `apply` updates the page and the local data, then a
    /// sync brings the rest (tasks of a left or deleted group, new members' profiles).
    fn run<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&supabase::Session) -> Result<T, Error> + Send + 'static,
        apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.confirm = None;
        cx.notify();
        let this: WeakEntity<Self> = cx.weak_entity();
        account::online(cx, work, move |result, cx: &mut App| {
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(value) => apply(this, value, cx),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
            sync::request(cx, sync::NOW);
        });
    }

    fn create_group(&mut self, cx: &mut Context<Self>) {
        let name = self.new_group.read(cx).text().trim().to_string();
        if name.is_empty() {
            return;
        }
        self.run(
            cx,
            move |session| supabase::create_group(session, &name),
            |this, group: Group, cx| {
                this.selected = Some(group.id);
                this.new_group.update(cx, |input, cx| input.set_text("", cx));
                AppState::global(cx).update(cx, |s, cx| s.apply_remote(cx, |d| d.groups.push(group)));
            },
        );
    }

    fn add_member(&mut self, cx: &mut Context<Self>) {
        let name = self.new_member.read(cx).text().trim().trim_start_matches('@').to_string();
        let Some(group) = self.selected else { return };
        if name.is_empty() {
            return;
        }
        self.run(
            cx,
            move |session| {
                let profile = supabase::find_profile(session, &name)?.ok_or_else(|| {
                    Error::Local(format!("\"{name}\" adıyla Odak'a giriş yapmış bir Discord kullanıcısı yok."))
                })?;
                match supabase::add_member(session, group, profile.id) {
                    Err(Error::Status(409, _)) => Err(Error::Local(format!("{} zaten bu grupta.", profile.name()))),
                    result => result.map(|()| profile),
                }
            },
            move |this, profile: Profile, cx| {
                this.new_member.update(cx, |input, cx| input.set_text("", cx));
                AppState::global(cx).update(cx, |s, cx| {
                    s.apply_remote(cx, |d| {
                        if let Some(g) = d.groups.iter_mut().find(|g| g.id == group) {
                            g.members.push(profile.id);
                        }
                        if d.profile(profile.id).is_none() {
                            d.profiles.push(profile);
                        }
                    })
                });
            },
        );
    }

    /// Removes a member (owner) or the account itself (leaving the group).
    fn remove(&mut self, group: Uuid, user: Uuid, cx: &mut Context<Self>) {
        let me = AppState::global(cx).read(cx).data.me();
        self.run(
            cx,
            move |session| supabase::remove_member(session, group, user),
            move |this, (), cx| {
                let left = Some(user) == me;
                if left {
                    this.selected = None;
                }
                AppState::global(cx).update(cx, |s, cx| {
                    s.apply_remote(cx, |d| {
                        if left {
                            d.groups.retain(|g| g.id != group);
                        } else if let Some(g) = d.groups.iter_mut().find(|g| g.id == group) {
                            g.members.retain(|m| *m != user);
                        }
                    })
                });
            },
        );
    }

    fn delete_group(&mut self, group: Uuid, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |session| supabase::delete_group(session, group),
            move |this, (), cx| {
                this.selected = None;
                AppState::global(cx).update(cx, |s, cx| s.apply_remote(cx, |d| d.groups.retain(|g| g.id != group)));
            },
        );
    }

    /// First click arms `confirm`, the second runs `action`.
    fn confirmed(&mut self, confirm: Confirm, cx: &mut Context<Self>) -> bool {
        if self.confirm == Some(confirm) {
            return true;
        }
        self.confirm = Some(confirm);
        cx.notify();
        false
    }

    fn group_list(&self, groups: &[Group], selected: Option<Uuid>, me: Option<Uuid>, c: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let hover = c.hover;
        div()
            .w(px(280.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(c.border)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(self.new_group.clone()))
                    .child(primary_button("create-group", "Oluştur", c).on_click(cx.listener(|this, _, _, cx| this.create_group(cx)))),
            )
            .when(groups.is_empty(), |d| {
                d.child(div().py_2().text_sm().text_color(c.muted).child("Henüz bir grubun yok. Bir ad yazıp oluştur."))
            })
            .children(groups.iter().map(|g| {
                let id = g.id;
                let active = selected == Some(id);
                let role = if Some(g.owner_id) == me { "Sahip" } else { "Üye" };
                div()
                    .id(id)
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(active, |d| d.bg(hover))
                    .hover(move |s| s.bg(hover))
                    .child(icon(icons::USERS).text_color(c.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(g.name.clone()))
                            .child(div().text_xs().text_color(c.muted).child(format!("{} üye · {role}", g.members.len()))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id);
                        this.confirm = None;
                        this.error = None;
                        cx.notify();
                    }))
            }))
    }

    fn members(&self, g: &Group, me: Option<Uuid>, c: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let data = &AppState::global(cx).read(cx).data;
        let owner = Some(g.owner_id) == me;
        let group = g.id;
        let danger = c.danger;
        // The owner first, then the others by name.
        let mut members = g.members.clone();
        members.sort_by_key(|m| (*m != g.owner_id, user_name(data, *m).to_lowercase()));
        let rows = members.into_iter().map(|user| {
            let confirming = self.confirm == Some(Confirm::Remove(user));
            let profile = data.profile(user);
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_1p5()
                .child(user_avatar(data, user, px(32.), c))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(user_name(data, user)))
                        .when_some(profile.filter(|p| !p.username.is_empty()), |d, p| {
                            d.child(div().text_xs().text_color(c.muted).child(format!("@{}", p.username)))
                        }),
                )
                .when(user == g.owner_id, |d| d.child(chip("Sahip", c.accent)))
                .when(owner && user != g.owner_id, |d| {
                    d.child(
                        button(user, if confirming { "Emin misin?" } else { "Çıkar" }, c)
                            .when(confirming, |b| b.bg(danger).border_color(danger).text_color(white()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.confirmed(Confirm::Remove(user), cx) {
                                    this.remove(group, user, cx);
                                }
                            })),
                    )
                })
        });
        let (end_label, end_confirm) = if owner {
            ("Grubu sil", Confirm::DeleteGroup(group))
        } else {
            ("Gruptan ayrıl", Confirm::Leave(group))
        };
        let confirming_end = self.confirm == Some(end_confirm);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .rounded_xl()
            .bg(c.surface)
            .border_1()
            .border_color(c.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(g.name.clone()))
                    .child(div().text_xs().text_color(c.muted).child(format!("{} üye", g.members.len()))),
            )
            .child(div().flex().flex_col().children(rows))
            .when(owner, |d| {
                d.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_1().child(self.new_member.clone()))
                        .child(
                            button("add-member", "Ekle", c)
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icon(icons::USER_ADD))
                                .on_click(cx.listener(|this, _, _, cx| this.add_member(cx))),
                        ),
                )
                .child(div().text_xs().text_color(c.muted).child(
                    "Eklemek istediğin kişinin en az bir kez Odak'a Discord ile giriş yapmış olması gerekir.",
                ))
            })
            .child(
                div().flex().items_center().gap_3().child(
                    button("end", if confirming_end { "Emin misin?" } else { end_label }, c)
                        .text_color(danger)
                        .when(confirming_end, |b| b.bg(danger).border_color(danger).text_color(white()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirmed(end_confirm, cx) {
                                match end_confirm {
                                    Confirm::DeleteGroup(g) => this.delete_group(g, cx),
                                    Confirm::Leave(g) => {
                                        if let Some(me) = me {
                                            this.remove(g, me, cx);
                                        }
                                    }
                                    Confirm::Remove(_) => {}
                                }
                            }
                        })),
                )
                .when(owner, |d| d.child(div().text_xs().text_color(c.muted).child("Grubun görevleri de silinir."))),
            )
    }
}

impl Render for GroupsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::current(cx);
        let state = AppState::global(cx);
        let (signed_in, groups, me) = {
            let s = state.read(cx);
            (matches!(s.auth, Auth::SignedIn(_)), s.data.groups.clone(), s.data.me())
        };
        let page = div().id("groups").size_full().overflow_y_scroll().child(
            div().p_6().flex().flex_col().gap_5().child(page_title("Gruplar", &c)),
        );
        if !supabase::enabled() || !signed_in {
            return page.child(
                div().px_6().flex().flex_col().items_start().gap_3().child(
                    div().text_sm().text_color(c.muted).child(if supabase::enabled() {
                        "Gruplar için Discord ile giriş yapman gerekiyor. Gruptaki herkes görevleri görür, düzenler ve birbirine atayabilir."
                    } else {
                        "Bu sürümde hesap özellikleri kapalı."
                    }),
                )
                .when(supabase::enabled(), |d| {
                    d.child(primary_button("groups-sign-in", "Discord ile giriş yap", &c).on_click(|_, _, cx| account::sign_in(cx)))
                }),
            );
        }
        // Keep a valid selection: the first group when none (or a gone one) is selected.
        let selected = self.selected.filter(|id| groups.iter().any(|g| g.id == *id)).or(groups.first().map(|g| g.id));
        self.selected = selected;
        let details = selected.and_then(|id| groups.iter().find(|g| g.id == id));
        page.child(
            div()
                .px_6()
                .pb_6()
                .flex()
                .flex_col()
                .gap_3()
                .when_some(self.error.clone(), |d, e| d.child(div().text_sm().text_color(c.danger).child(e)))
                .when(self.busy, |d| d.child(div().text_sm().text_color(c.muted).child("İşleniyor...")))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap_4()
                        .child(self.group_list(&groups, selected, me, &c, cx))
                        .map(|d| match details {
                            Some(g) => d.child(self.members(g, me, &c, cx)),
                            None => d.child(
                                div().flex_1().p_4().text_sm().text_color(c.muted).child("Bir grup oluşturunca üyeleri burada görünür."),
                            ),
                        }),
                ),
        )
    }
}
