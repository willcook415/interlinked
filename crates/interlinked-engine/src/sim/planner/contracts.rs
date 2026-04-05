use crate::sim::types::TripPurpose;

pub(super) fn purpose_index(purpose: TripPurpose) -> usize {
    match purpose {
        TripPurpose::Work => 0,
        TripPurpose::Education => 1,
        TripPurpose::Shopping => 2,
        TripPurpose::Leisure => 3,
        TripPurpose::Essential => 4,
        TripPurpose::Intercity => 5,
    }
}

pub(super) fn purpose_from_index(idx: usize) -> TripPurpose {
    match idx {
        0 => TripPurpose::Work,
        1 => TripPurpose::Education,
        2 => TripPurpose::Shopping,
        3 => TripPurpose::Leisure,
        4 => TripPurpose::Essential,
        _ => TripPurpose::Intercity,
    }
}
