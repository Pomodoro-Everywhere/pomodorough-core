use super::{COMMAND, HISTORY, TIMER};
use crate::strict_json::shape::{Field, Shape};

pub(crate) const FINISH: Shape = Shape::Record(&[
    Field::required("command", COMMAND),
    Field::required("sourceTimer", TIMER),
    Field::required("sourceHistory", HISTORY),
]);
