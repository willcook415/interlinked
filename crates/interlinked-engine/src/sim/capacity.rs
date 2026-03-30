use super::types::BoardingTimeBin;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct BoardingSimulationInput {
    pub attempted_total: f64,
    pub headway_s: f64,
    pub veh_cap: f64,
    pub period_s: f64,
    pub bin_s: f64,
    pub max_extra_wait_s: f64,
    pub initial_queue: f64,
    pub initial_time_to_next_departure_s: f64,
}

#[allow(dead_code)]
pub(crate) fn simulate_boarding_time_sliced(
    input: BoardingSimulationInput,
) -> (f64, f64, f64, f64, f64, f64, Vec<BoardingTimeBin>, f64) {
    let BoardingSimulationInput {
        attempted_total,
        headway_s,
        veh_cap,
        period_s,
        bin_s,
        max_extra_wait_s,
        initial_queue,
        initial_time_to_next_departure_s,
    } = input;

    if attempted_total <= 0.0 || headway_s <= 0.0 || veh_cap <= 0.0 || period_s <= 0.0 {
        return (
            0.0,
            attempted_total.max(0.0),
            attempted_total.max(0.0),
            0.0,
            0.0,
            0.0,
            vec![],
            headway_s.max(0.0),
        );
    }

    let bin_s = bin_s.max(1.0);
    let n_bins = (period_s / bin_s).ceil().max(1.0) as usize;

    let arrivals_per_bin = attempted_total / (n_bins as f64);

    let mut queue = initial_queue.max(0.0);
    let mut served_total = 0.0_f64;
    let mut bins: Vec<BoardingTimeBin> = Vec::with_capacity(n_bins);
    let mut dep_total: usize = 0;

    // Time until next departure (first departure after one headway)
    // Persisted phase (how long until the next departure when the period starts).
    // Normalize into [0, headway_s] to avoid drifting.
    let mut time_to_next = if initial_time_to_next_departure_s.is_finite() {
        let init = initial_time_to_next_departure_s;

        // Convention:
        // - 0.0  => departure is due right now
        // - headway_s => full headway until next departure
        if init <= 0.0 {
            0.0
        } else {
            let mut x = init % headway_s;
            if x < 0.0 {
                x += headway_s;
            }

            // If init is exactly a multiple of headway (including init == headway),
            // we want "full headway remaining", not "departure now".
            if x <= 1e-9 {
                headway_s
            } else {
                x
            }
        }
    } else {
        headway_s
    };

    for i in 0..n_bins {
        queue += arrivals_per_bin;

        // Count departures that occur within this bin
        let mut remaining = bin_s;
        let mut dep_i: usize = 0;

        while time_to_next <= remaining {
            // If a departure is scheduled exactly "now", count it and reset the headway
            if time_to_next <= 1e-9 {
                dep_i += 1;
                time_to_next = headway_s;
                continue;
            }

            dep_i += 1;
            remaining -= time_to_next;
            time_to_next = headway_s;
        }

        // Advance time within the bin (no departure if time_to_next > remaining)
        time_to_next = (time_to_next - remaining).max(0.0);
        dep_total += dep_i;
        let cap_bin = (dep_i as f64) * veh_cap;

        let served_bin = queue.min(cap_bin);
        queue -= served_bin;
        served_total += served_bin;

        bins.push(BoardingTimeBin {
            bin_index: i,
            arrivals: arrivals_per_bin,
            served: served_bin,
            queue_end: queue,
            departures: dep_i,
            capacity: cap_bin,
        });
    }

    let departures_in_period = dep_total as f64;
    let capacity_in_period = departures_in_period * veh_cap;
    let denied = queue.max(0.0);
    let queue_end = denied;

    let mut extra_wait_s = 0.0;
    if queue_end > 0.0 {
        extra_wait_s = headway_s * (queue_end / veh_cap);
        extra_wait_s = extra_wait_s.min(max_extra_wait_s.max(0.0));
    }

    (
        served_total,
        denied,
        queue_end,
        extra_wait_s,
        departures_in_period,
        capacity_in_period,
        bins,
        time_to_next,
    )
}
