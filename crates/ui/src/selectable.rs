use crate::theme::{Element, Theme};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::theme::palette::Extended;
use iced::widget::table::Column;
use iced::{Color, Event, Length, Rectangle, Renderer, Size, Vector, mouse, touch};

const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 2.0;

/// A table whose rows are lit and chosen as wholes: the row under the pointer is shaded, the chosen
/// one is marked, and pressing anywhere on a row sends `pick` for it. A control inside a cell keeps
/// its own presses.
pub fn table<'a, 'b, T: Clone, Message: Clone + 'a>(
    columns: impl IntoIterator<Item = Column<'a, 'b, T, Message, Theme>>,
    rows: Vec<T>,
    pick: impl Fn(&T) -> Message,
    chosen: impl Fn(&T) -> bool,
) -> Element<'a, Message> {
    let picks = rows.iter().map(pick).collect();
    let selected = rows.iter().position(chosen);
    let columns: Vec<_> = columns.into_iter().collect();
    let width = columns.len();
    let table = iced::widget::table(columns, rows)
        .padding_x(PADDING_X)
        .padding_y(PADDING_Y);
    Element::new(Selectable {
        table: table.into(),
        columns: width,
        picks,
        selected,
    })
}

struct Selectable<'a, Message> {
    table: Element<'a, Message>,
    columns: usize,
    picks: Vec<Message>,
    selected: Option<usize>,
}

/// The row the pointer was over when the table last saw an event, so a change redraws it.
#[derive(Default)]
struct State {
    hovered: Option<usize>,
}

/// The strip each row covers, as wide as the table and without the heading, from the bounds of the
/// table's cells in reading order.
fn strips(table: Rectangle, cells: &[Rectangle], columns: usize) -> Vec<Rectangle> {
    cells
        .chunks(columns)
        .skip(1)
        .map(|row| {
            let (top, bottom) = row
                .iter()
                .fold((f32::MAX, f32::MIN), |(top, bottom), cell| {
                    (top.min(cell.y), bottom.max(cell.y + cell.height))
                });
            Rectangle {
                x: table.x,
                y: top - PADDING_Y,
                width: table.width,
                height: bottom - top + PADDING_Y * 2.0,
            }
        })
        .collect()
}

/// What to shade, in drawing order: the row under the pointer, then the chosen row, which keeps its
/// own shade when the pointer is on it.
fn shaded(
    rows: &[Rectangle],
    hovered: Option<usize>,
    selected: Option<usize>,
    palette: &Extended,
) -> Vec<(Rectangle, Color)> {
    [
        (
            hovered.filter(|row| Some(*row) != selected),
            palette.background.strong.color,
        ),
        (selected, palette.primary.weak.color),
    ]
    .into_iter()
    .filter_map(|(row, color)| Some((*rows.get(row?)?, color)))
    .collect()
}

impl<Message> Selectable<'_, Message> {
    fn strips(&self, layout: Layout<'_>) -> Vec<Rectangle> {
        let cells: Vec<Rectangle> = layout.children().map(|cell| cell.bounds()).collect();
        strips(layout.bounds(), &cells, self.columns)
    }

    fn row_at(&self, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
        let point = cursor.position_over(layout.bounds())?;
        self.strips(layout)
            .iter()
            .position(|row| row.contains(point))
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Selectable<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.table)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.table));
    }

    fn size(&self) -> Size<Length> {
        self.table.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.table
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.table
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.table.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let hovered = self.row_at(layout, cursor);
        let state = tree.state.downcast_mut::<State>();
        if state.hovered != hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }

        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );
        if pressed
            && !shell.is_event_captured()
            && let Some(row) = hovered
        {
            shell.publish(self.picks[row].clone());
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let inside = self.table.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        if inside == mouse::Interaction::None && self.row_at(layout, cursor).is_some() {
            mouse::Interaction::Pointer
        } else {
            inside
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let rows = self.strips(layout);
        let hovered = cursor
            .position_over(layout.bounds())
            .and_then(|point| rows.iter().position(|row| row.contains(point)));
        let mut palette = *theme.extended_palette();
        palette.background.strong.color = theme.color("row_hover", palette.background.strong.color);
        palette.primary.weak.color = theme.color("row_selected", palette.primary.weak.color);
        for (bounds, color) in shaded(&rows, hovered, self.selected, &palette) {
            renderer::Renderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds,
                    ..renderer::Quad::default()
                },
                color,
            );
        }
        self.table.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.table.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::{button, table::column, text};
    use iced::{Fill, Point};
    use iced_test::Simulator;
    use iced_test::simulator::click;

    use super::*;
    use crate::widgets::pressed;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Pick(&'static str),
        Toggle(&'static str),
    }

    type Person = (&'static str, &'static str);

    fn people<'a>(chosen: Option<&'static str>) -> Element<'a, Message> {
        let columns = [
            column(text("Name"), |person: Person| text(person.0)).width(Fill),
            column(text("Role"), |person: Person| text(person.1)),
            column(text("Action"), |person: Person| {
                button(text(format!("Toggle {}", person.0))).on_press(Message::Toggle(person.0))
            }),
        ];
        table(
            columns,
            vec![("alpha", "admin"), ("beta", "guest")],
            |person| Message::Pick(person.0),
            |person| Some(person.0) == chosen,
        )
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn pressing_any_cell_of_a_row_picks_that_row() {
        assert_eq!(pressed(people(None), "admin"), [Message::Pick("alpha")]);
        assert_eq!(pressed(people(None), "guest"), [Message::Pick("beta")]);
    }

    #[test]
    fn pressing_beside_the_cells_of_a_row_still_picks_it() {
        let mut simulator = Simulator::with_size(
            iced::Settings::default(),
            Size::new(600.0, 400.0),
            people(None),
        );
        let beta = simulator.find("beta").unwrap().bounds();
        simulator.point_at(Point::new(1.0, beta.center_y()));
        let _ = simulator.simulate(click());
        assert_eq!(
            simulator.into_messages().collect::<Vec<_>>(),
            [Message::Pick("beta")]
        );
    }

    #[test]
    fn the_headings_are_not_a_row() {
        assert_eq!(pressed(people(None), "Role"), []);
    }

    #[test]
    fn a_control_in_a_cell_keeps_its_press_and_does_not_pick_the_row() {
        assert_eq!(
            pressed(people(None), "Toggle beta"),
            [Message::Toggle("beta")]
        );
    }

    #[test]
    fn each_row_is_a_strip_as_wide_as_the_table_that_takes_in_the_padding_around_its_tallest_cell()
    {
        let table = rect(0.0, 0.0, 300.0, 100.0);
        let cells = [
            rect(10.0, 2.0, 40.0, 20.0),
            rect(100.0, 2.0, 50.0, 20.0),
            rect(10.0, 25.0, 40.0, 18.0),
            rect(100.0, 25.0, 50.0, 18.0),
            rect(10.0, 46.0, 40.0, 18.0),
            rect(100.0, 46.0, 50.0, 30.0),
        ];
        assert_eq!(
            strips(table, &cells, 2),
            [rect(0.0, 23.0, 300.0, 22.0), rect(0.0, 44.0, 300.0, 34.0)]
        );
    }

    #[test]
    fn the_hovered_row_is_shaded_and_the_chosen_one_is_marked_over_it() {
        let rows = [rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 12.0, 10.0, 10.0)];
        let theme = Theme::Dark;
        let palette = theme.extended_palette();
        let (hover, mark) = (palette.background.strong.color, palette.primary.weak.color);
        assert_eq!(shaded(&rows, None, None, palette), []);
        assert_eq!(shaded(&rows, Some(0), None, palette), [(rows[0], hover)]);
        assert_eq!(
            shaded(&rows, Some(0), Some(1), palette),
            [(rows[0], hover), (rows[1], mark)]
        );
        assert_eq!(shaded(&rows, Some(1), Some(1), palette), [(rows[1], mark)]);
        assert_eq!(shaded(&rows, None, Some(5), palette), []);
    }
}
