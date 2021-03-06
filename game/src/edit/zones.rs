use crate::app::App;
use crate::common::ColorDiscrete;
use crate::common::CommonState;
use crate::edit::apply_map_edits;
use crate::edit::select::RoadSelector;
use crate::game::{State, Transition};
use crate::helpers::{checkbox_per_mode, intersections_from_roads};
use enumset::EnumSet;
use ezgui::{
    hotkey, Btn, Color, Composite, Drawable, EventCtx, GfxCtx, HorizontalAlignment, Key, Line,
    Outcome, Text, VerticalAlignment, Widget,
};
use map_model::{EditCmd, RoadID};
use maplit::btreeset;
use sim::TripMode;
use std::collections::BTreeSet;

pub struct ZoneEditor {
    composite: Composite,
    selector: RoadSelector,
    allow_through_traffic: BTreeSet<TripMode>,
    unzoomed: Drawable,
    zoomed: Drawable,

    orig_members: BTreeSet<RoadID>,
}

impl ZoneEditor {
    pub fn new(ctx: &mut EventCtx, app: &mut App, start: RoadID) -> Box<dyn State> {
        let start = app.primary.map.get_r(start);
        let members = if let Some(z) = start.get_zone(&app.primary.map) {
            z.members.clone()
        } else {
            // Starting a new zone
            btreeset! { start.id }
        };
        let allow_through_traffic = start
            .allow_through_traffic
            .into_iter()
            .map(|c| TripMode::from_constraints(c))
            .collect();

        let (unzoomed, zoomed, legend) = draw_zone(ctx, app, &members);
        let orig_members = members.clone();
        let selector = RoadSelector::new(app, members);

        Box::new(ZoneEditor {
            composite: Composite::new(Widget::col(vec![
                Line("Editing restricted access zone")
                    .small_heading()
                    .draw(ctx),
                selector.make_controls(ctx).named("selector"),
                legend,
                make_instructions(ctx, &allow_through_traffic),
                checkbox_per_mode(ctx, app, &allow_through_traffic),
                Widget::custom_row(vec![
                    Btn::text_fg("Apply").build_def(ctx, hotkey(Key::Enter)),
                    Btn::text_fg("Cancel").build_def(ctx, hotkey(Key::Escape)),
                ])
                .evenly_spaced(),
            ]))
            .aligned(HorizontalAlignment::Center, VerticalAlignment::Top)
            .build(ctx),
            orig_members,
            selector,
            allow_through_traffic,
            unzoomed,
            zoomed,
        })
    }
}

// TODO Handle splitting/merging zones.
impl State for ZoneEditor {
    fn event(&mut self, ctx: &mut EventCtx, app: &mut App) -> Transition {
        match self.composite.event(ctx) {
            Some(Outcome::Clicked(x)) => match x.as_ref() {
                "Apply" => {
                    let mut edits = app.primary.map.get_edits().clone();
                    for r in self.orig_members.difference(&self.selector.roads) {
                        edits.commands.push(EditCmd::ChangeAccessRestrictions {
                            id: *r,
                            old_allow_through_traffic: app
                                .primary
                                .map
                                .get_r(*r)
                                .allow_through_traffic
                                .clone(),
                            new_allow_through_traffic: EnumSet::all(),
                        });
                    }

                    let new_allow_through_traffic = self
                        .allow_through_traffic
                        .iter()
                        .map(|m| m.to_constraints())
                        .collect::<EnumSet<_>>();
                    for r in &self.selector.roads {
                        let old_allow_through_traffic =
                            app.primary.map.get_r(*r).allow_through_traffic.clone();
                        if old_allow_through_traffic != new_allow_through_traffic {
                            edits.commands.push(EditCmd::ChangeAccessRestrictions {
                                id: *r,
                                old_allow_through_traffic,
                                new_allow_through_traffic: new_allow_through_traffic.clone(),
                            });
                        }
                    }

                    apply_map_edits(ctx, app, edits);
                    return Transition::Pop;
                }
                "Cancel" => {
                    return Transition::Pop;
                }
                x => {
                    if self.selector.event(ctx, app, Some(x)) {
                        let new_controls = self
                            .selector
                            .make_controls(ctx)
                            .named("selector")
                            .margin_below(10);
                        self.composite.replace(ctx, "selector", new_controls);
                        let (unzoomed, zoomed, _) = draw_zone(ctx, app, &self.selector.roads);
                        self.unzoomed = unzoomed;
                        self.zoomed = zoomed;
                    }
                }
            },
            None => {
                if self.selector.event(ctx, app, None) {
                    let new_controls = self
                        .selector
                        .make_controls(ctx)
                        .named("selector")
                        .margin_below(10);
                    self.composite.replace(ctx, "selector", new_controls);
                    let (unzoomed, zoomed, _) = draw_zone(ctx, app, &self.selector.roads);
                    self.unzoomed = unzoomed;
                    self.zoomed = zoomed;
                }
            }
        }

        let mut new_allow_through_traffic = BTreeSet::new();
        for m in TripMode::all() {
            if self.composite.is_checked(m.ongoing_verb()) {
                new_allow_through_traffic.insert(m);
            }
        }
        if self.allow_through_traffic != new_allow_through_traffic {
            let instructions = make_instructions(ctx, &new_allow_through_traffic);
            self.composite.replace(ctx, "instructions", instructions);
            self.allow_through_traffic = new_allow_through_traffic;
        }

        Transition::Keep
    }

    fn draw(&self, g: &mut GfxCtx, app: &App) {
        // TODO The currently selected road is covered up pretty badly
        if g.canvas.cam_zoom < app.opts.min_zoom_for_detail {
            g.redraw(&self.unzoomed);
        } else {
            g.redraw(&self.zoomed);
        }
        self.composite.draw(g);
        self.selector.draw(g, app, false);
        CommonState::draw_osd(g, app);
    }
}

fn draw_zone(
    ctx: &mut EventCtx,
    app: &App,
    members: &BTreeSet<RoadID>,
) -> (Drawable, Drawable, Widget) {
    let mut colorer = ColorDiscrete::new(
        app,
        vec![
            ("restricted road", Color::CYAN),
            ("entrance/exit", Color::BLUE),
        ],
    );
    let map = &app.primary.map;
    for r in members {
        let r = map.get_r(*r);
        colorer.add_r(r.id, "restricted road");
        for next in map.get_next_roads(r.id) {
            if !members.contains(&next) {
                colorer.add_i(r.common_endpt(map.get_r(next)), "entrance/exit");
            }
        }
    }
    for i in intersections_from_roads(members, &app.primary.map) {
        colorer.add_i(i, "restricted road");
    }
    colorer.build(ctx)
}

fn make_instructions(ctx: &mut EventCtx, allow_through_traffic: &BTreeSet<TripMode>) -> Widget {
    if allow_through_traffic == &TripMode::all().into_iter().collect() {
        Text::from(Line(
            "Through-traffic is allowed for everyone, meaning this is just a normal public road. \
             Would you like to restrict it?",
        ))
        .wrap_to_pct(ctx, 30)
        .draw(ctx)
    } else {
        Line("Trips may start or end in this zone, but through-traffic is only allowed for:")
            .draw(ctx)
    }
    .margin_below(10)
    .named("instructions")
}
