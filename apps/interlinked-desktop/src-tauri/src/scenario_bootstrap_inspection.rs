use crate::*;

pub(crate) fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let qq = q.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f64 * qq).round() as usize;
    sorted[idx]
}

pub(crate) fn default_params() -> Params {
    Params {
        walk_weight: 1.0,
        wait_weight: 2.0,
        ivt_weight: 1.0,
        transfer_penalty_s: 300.0,
        access_walk_speed_mps: 1.4,
        access_radius_m: 1200.0,
        gravity_beta: 0.0003,
        trips_per_person: 1.0,
        purpose_share_home_work: 0.52,
        purpose_share_home_education: 0.12,
        purpose_share_home_retail: 0.18,
        purpose_share_home_recreation: 0.10,
        purpose_share_other: 0.08,
        attraction_weight_office: 1.0,
        attraction_weight_retail: 0.9,
        attraction_weight_recreation: 0.7,
        attraction_weight_industrial: 1.1,
        attraction_weight_education: 0.8,
        attraction_weight_health: 0.75,
        route_choice_k: 3,
        route_choice_theta: 0.002,
        assignment_max_iters: 8,
        assignment_convergence_rel: 0.01,
        capacity_enabled: true,
        queue_max_extra_wait_s: 3600.0,
        fare_enabled: true,
        fare_value_of_time_base_per_hour: 12.0,
        fare_elasticity: 0.35,
        fare_reference_base: 2.5,
        fare_transfer_window_s: 2700.0,
        fare_free_transfers_per_trip: 1,
        fare_overflow_retry_share: 0.15,
        fare_mode_bus_base: 1.8,
        fare_mode_tram_base: 2.3,
        fare_mode_metro_base: 2.7,
        fare_mode_rail_base: 3.6,
        fare_mode_ferry_base: 3.0,
        fare_mode_default_base: 2.5,
        station_capacity_scale_boarding: 1.0,
        station_capacity_scale_alighting: 1.0,
        station_queue_capacity_scale: 1.0,
        debug_sample_origin_zone: None,
        debug_sample_dest_zone: None,
        demand_profile: vec![],
        demand_purpose_profile: vec![],
    }
}

pub(crate) fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

pub(crate) fn stable_noise_01(a: i32, b: i32, k: f64) -> f64 {
    let n = (a as f64 * 12.9898 + b as f64 * 78.233 + k * 437.585453).sin() * 43758.5453123;
    let frac = n - n.floor();
    if frac.is_finite() {
        frac
    } else {
        0.5
    }
}

pub(crate) fn synthesize_city_demand(
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> (Vec<Zone>, Vec<DemandCell>) {
    let (x, y) = lonlat_to_web_mercator_m(center_lon, center_lat);
    let city_pop = city_population
        .unwrap_or(750_000)
        .clamp(120_000, 30_000_000) as f64;
    let residents_total = city_pop * 1.35;
    let employment_ratio = (0.36 + (city_pop.log10() - 5.0) * 0.08).clamp(0.32, 0.52);
    let jobs_total = residents_total * employment_ratio;
    let phase = center_lon * 0.31 + center_lat * 0.23;
    let city_scale = (city_pop / 750_000.0).powf(0.35).clamp(0.65, 3.0);
    let radius_cells = (4.0 + 2.0 * city_scale).round().clamp(4.0, 9.0) as i32;
    let hex_size_m = (640.0 * city_scale.powf(0.35)).clamp(560.0, 980.0);
    let sqrt3 = 3.0_f64.sqrt();
    let spread_m = (radius_cells as f64 * hex_size_m * 1.8).max(1.0);
    let country = country_iso2
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2);

    #[derive(Clone)]
    struct CellDraft {
        cell_id: String,
        x: f64,
        y: f64,
        residents_weight: f64,
        jobs_weight: f64,
        residential: f64,
        office: f64,
        retail: f64,
        recreation: f64,
        industrial: f64,
        education: f64,
        health: f64,
        centrality: f64,
    }

    let mut drafts = Vec::<CellDraft>::new();
    let mut residents_weight_sum = 0.0;
    let mut jobs_weight_sum = 0.0;

    for q in -radius_cells..=radius_cells {
        let r_min = (-radius_cells).max(-q - radius_cells);
        let r_max = radius_cells.min(-q + radius_cells);
        for r in r_min..=r_max {
            let qf = q as f64;
            let rf = r as f64;
            let px = hex_size_m * (sqrt3 * qf + 0.5 * sqrt3 * rf);
            let py = hex_size_m * (1.5 * rf);
            let jx = (stable_noise_01(q, r, phase + 1.7) - 0.5) * hex_size_m * 0.55;
            let jy = (stable_noise_01(q, r, phase - 2.1) - 0.5) * hex_size_m * 0.55;
            let dx = px + jx;
            let dy = py + jy;

            let ux = dx / spread_m;
            let uy = dy / spread_m;
            let rr = (ux * ux + uy * uy).sqrt();
            let angle = uy.atan2(ux);

            let cbd = (-(ux * ux + uy * uy) / 0.06).exp();
            let c1x = 0.42 * phase.cos();
            let c1y = 0.42 * phase.sin();
            let c2x = 0.56 * (phase + 2.25).cos();
            let c2y = 0.56 * (phase + 2.25).sin();
            let c3x = 0.50 * (phase - 1.95).cos();
            let c3y = 0.50 * (phase - 1.95).sin();
            let sub_center = ((-((ux - c1x).powi(2) + (uy - c1y).powi(2)) / 0.030).exp()
                + (-((ux - c2x).powi(2) + (uy - c2y).powi(2)) / 0.040).exp()
                + (-((ux - c3x).powi(2) + (uy - c3y).powi(2)) / 0.045).exp())
                / 3.0;
            let inner_ring = (-((rr - 0.34).powi(2)) / 0.030).exp();
            let residential_belt = (-((rr - 0.70).powi(2)) / 0.065).exp();
            let periphery = clamp01((rr - 0.58) / 0.45);
            let corridor = (1.0
                + 0.34 * (2.0 * angle + phase).cos()
                + 0.18 * (3.0 * angle - 0.7 * phase).sin())
            .max(0.18);

            let residents_weight = (0.53 * residential_belt
                + 0.20 * periphery
                + 0.13 * inner_ring
                + 0.08 * corridor
                + 0.06 * (1.0 - cbd)
                + 0.04 * (1.0 - sub_center))
                .max(0.01);
            let jobs_weight = (0.56 * cbd
                + 0.24 * sub_center
                + 0.10 * inner_ring
                + 0.07 * corridor
                + 0.03 * (1.0 - periphery))
                .max(0.01);

            let mut residential = (0.48 + 0.50 * residential_belt + 0.16 * periphery
                - 0.30 * cbd
                - 0.08 * sub_center)
                .max(0.01);
            let mut office = (0.06 + 0.92 * cbd + 0.52 * sub_center + 0.08 * corridor
                - 0.22 * residential_belt)
                .max(0.01);
            let mut retail =
                (0.05 + 0.34 * inner_ring + 0.25 * corridor + 0.24 * sub_center + 0.14 * cbd)
                    .max(0.01);
            let mut recreation =
                (0.04 + 0.24 * residential_belt + 0.16 * periphery + 0.08 * (1.0 - rr).max(0.0))
                    .max(0.01);
            let mut industrial =
                (0.04 + 0.30 * periphery + 0.18 * corridor + 0.10 * (1.0 - cbd)).max(0.01);
            let mut education =
                (0.04 + 0.16 * inner_ring + 0.10 * residential_belt + 0.05 * sub_center).max(0.01);
            let mut health =
                (0.03 + 0.14 * cbd + 0.12 * inner_ring + 0.08 * residential_belt).max(0.01);
            let mix_sum =
                residential + office + retail + recreation + industrial + education + health;
            residential /= mix_sum;
            office /= mix_sum;
            retail /= mix_sum;
            recreation /= mix_sum;
            industrial /= mix_sum;
            education /= mix_sum;
            health /= mix_sum;
            let centrality = clamp01(0.60 * cbd + 0.27 * sub_center + 0.13 * (corridor / 1.52));

            residents_weight_sum += residents_weight;
            jobs_weight_sum += jobs_weight;
            drafts.push(CellDraft {
                cell_id: format!("dc:{q}:{r}"),
                x: x + dx,
                y: y + dy,
                residents_weight,
                jobs_weight,
                residential,
                office,
                retail,
                recreation,
                industrial,
                education,
                health,
                centrality,
            });
        }
    }

    let mut zones = Vec::<Zone>::with_capacity(drafts.len());
    let mut demand_cells = Vec::<DemandCell>::with_capacity(drafts.len());
    for d in drafts {
        let residents_night =
            (residents_total * d.residents_weight / residents_weight_sum).max(50.0);
        let jobs_day = (jobs_total * d.jobs_weight / jobs_weight_sum).max(20.0);
        zones.push(Zone {
            id: format!("z:{}", d.cell_id),
            x: d.x,
            y: d.y,
            population: residents_night,
            jobs: jobs_day,
            country_iso2: country.clone(),
        });
        demand_cells.push(DemandCell {
            cell_id: d.cell_id,
            x: d.x,
            y: d.y,
            area_m2: (3.0 * sqrt3 / 2.0) * hex_size_m * hex_size_m,
            residents_night,
            jobs_day,
            activity_mix_residential: d.residential,
            activity_mix_office: d.office,
            activity_mix_retail: d.retail,
            activity_mix_recreation: d.recreation,
            activity_mix_industrial: d.industrial,
            activity_mix_education: d.education,
            activity_mix_health: d.health,
            centrality_score: d.centrality,
            data_quality_score: 0.72,
            country_iso2: country.clone(),
        });
    }
    (zones, demand_cells)
}

#[allow(dead_code)]
pub(crate) fn looks_like_legacy_lattice(scenario: &Scenario) -> bool {
    if scenario.world.demand_cells.len() != 81 {
        return false;
    }
    if !scenario
        .world
        .demand_cells
        .iter()
        .all(|c| c.cell_id.starts_with("df:"))
    {
        return false;
    }
    let mut xs = BTreeSet::<i64>::new();
    let mut ys = BTreeSet::<i64>::new();
    for c in &scenario.world.demand_cells {
        xs.insert((c.x / 100.0).round() as i64);
        ys.insert((c.y / 100.0).round() as i64);
    }
    xs.len() == 9 && ys.len() == 9
}

#[allow(dead_code)]
pub(crate) fn synthesize_country_demand(
    app: &AppHandle,
    country_iso2: &str,
    start_location: Option<&StartLocation>,
) -> Result<(Vec<Zone>, Vec<DemandCell>), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two letters".to_string());
    }

    let mut cities = list_cities_internal(app, &iso)?;
    if let Some(start) = start_location {
        if !cities.iter().any(|c| c.geonameid == start.city_id) {
            cities.push(CityOption {
                geonameid: start.city_id,
                name: start.city_name.clone(),
                lat: start.city_lat,
                lon: start.city_lon,
                population: start.city_population.unwrap_or(250_000),
            });
        }
    }
    if cities.is_empty() {
        return Err(format!("no city catalog rows for country {iso}"));
    }
    cities.sort_by(|a, b| {
        b.population
            .cmp(&a.population)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut major = cities
        .into_iter()
        .filter(|c| {
            c.population >= 20_000 || Some(c.geonameid) == start_location.map(|s| s.city_id)
        })
        .collect::<Vec<_>>();
    if major.is_empty() {
        return Err(format!("no usable city rows for country {iso}"));
    }
    major.truncate(180);
    let top_pop = major
        .first()
        .map(|c| c.population)
        .unwrap_or(500_000)
        .max(1) as f64;
    let golden = 2.399963229728653_f64;

    #[derive(Clone)]
    struct LocalDraft {
        x: f64,
        y: f64,
        residents_weight: f64,
        jobs_weight: f64,
        residential: f64,
        office: f64,
        retail: f64,
        recreation: f64,
        industrial: f64,
        education: f64,
        health: f64,
        centrality: f64,
    }

    let mut zones = Vec::<Zone>::new();
    let mut demand_cells = Vec::<DemandCell>::new();

    for (city_rank, city) in major.iter().enumerate() {
        let pop = city.population.max(30_000) as f64;
        let city_scale = (pop / 300_000.0).powf(0.36).clamp(0.55, 3.2);
        let city_weight = (pop / top_pop).powf(0.52).clamp(0.12, 1.0);
        let n_cells = ((20.0 + 30.0 * city_scale) * (0.72 + 0.28 * city_weight))
            .round()
            .clamp(18.0, 94.0) as usize;
        let radius_m = (11_000.0 + 26_000.0 * city_scale).clamp(10_000.0, 58_000.0);
        let phase = city.lon * 0.35 + city.lat * 0.24 + city.geonameid as f64 * 0.0000003;
        let city_residents_total = pop * (1.18 + 0.08 * city_weight);
        let employment_ratio = (0.34 + (pop.log10() - 5.0) * 0.07).clamp(0.26, 0.56);
        let city_jobs_total = city_residents_total * employment_ratio;
        let (cx, cy) = lonlat_to_web_mercator_m(city.lon, city.lat);

        let mut local = Vec::<LocalDraft>::with_capacity(n_cells);
        let mut residents_sum = 0.0;
        let mut jobs_sum = 0.0;
        for i in 0..n_cells {
            let t = (i as f64 + 0.5) / n_cells as f64;
            let spiral_r = radius_m * t.sqrt();
            let theta = i as f64 * golden + phase;
            let radial_jitter =
                (stable_noise_01(i as i32, city_rank as i32, phase) - 0.5) * radius_m * 0.08;
            let theta_jitter =
                (stable_noise_01(city.geonameid as i32, i as i32, phase + 1.13) - 0.5) * 0.28;
            let r = (spiral_r + radial_jitter).max(radius_m * 0.035);
            let dx = r * (theta + theta_jitter).cos();
            let dy = r * (theta + theta_jitter).sin();

            let u = (r / radius_m).clamp(0.0, 1.5);
            let cbd = (-4.6 * u * u).exp();
            let inner_ring = (-((u - 0.42).powi(2)) / 0.050).exp();
            let suburban = (-((u - 0.75).powi(2)) / 0.085).exp();
            let periphery = clamp01((u - 0.60) / 0.45);
            let corridor = (1.0
                + 0.30 * (2.0 * theta + phase).cos()
                + 0.17 * (3.0 * theta - 0.8 * phase).sin())
            .max(0.22);

            let residents_weight = (0.50 * suburban
                + 0.22 * periphery
                + 0.15 * inner_ring
                + 0.08 * corridor
                + 0.05 * (1.0 - cbd))
                .max(0.01);
            let jobs_weight =
                (0.57 * cbd + 0.20 * inner_ring + 0.15 * corridor + 0.08 * suburban).max(0.01);

            let mut residential =
                (0.50 + 0.46 * suburban + 0.14 * periphery - 0.30 * cbd).max(0.01);
            let mut office = (0.06 + 0.94 * cbd + 0.16 * corridor - 0.20 * suburban).max(0.01);
            let mut retail = (0.05 + 0.31 * inner_ring + 0.27 * corridor + 0.12 * cbd).max(0.01);
            let mut recreation = (0.04 + 0.24 * suburban + 0.16 * periphery).max(0.01);
            let mut industrial =
                (0.04 + 0.31 * periphery + 0.17 * corridor + 0.09 * (1.0 - cbd)).max(0.01);
            let mut education = (0.04 + 0.14 * inner_ring + 0.09 * suburban).max(0.01);
            let mut health = (0.03 + 0.12 * cbd + 0.10 * inner_ring + 0.07 * suburban).max(0.01);
            let mix_sum =
                residential + office + retail + recreation + industrial + education + health;
            residential /= mix_sum;
            office /= mix_sum;
            retail /= mix_sum;
            recreation /= mix_sum;
            industrial /= mix_sum;
            education /= mix_sum;
            health /= mix_sum;
            let centrality = clamp01(0.64 * cbd + 0.24 * (corridor / 1.47) + 0.12 * inner_ring);

            residents_sum += residents_weight;
            jobs_sum += jobs_weight;
            local.push(LocalDraft {
                x: cx + dx,
                y: cy + dy,
                residents_weight,
                jobs_weight,
                residential,
                office,
                retail,
                recreation,
                industrial,
                education,
                health,
                centrality,
            });
        }

        let area_m2 = (std::f64::consts::PI * radius_m * radius_m / n_cells as f64).max(40_000.0);
        let quality = (0.56 + 0.30 * city_weight).clamp(0.56, 0.9);
        for (i, d) in local.into_iter().enumerate() {
            let residents_night =
                (city_residents_total * d.residents_weight / residents_sum).max(30.0);
            let jobs_day = (city_jobs_total * d.jobs_weight / jobs_sum).max(12.0);
            let cell_id = format!("dc:{iso}:{}:{i}", city.geonameid);
            zones.push(Zone {
                id: format!("z:{cell_id}"),
                x: d.x,
                y: d.y,
                population: residents_night,
                jobs: jobs_day,
                country_iso2: Some(iso.clone()),
            });
            demand_cells.push(DemandCell {
                cell_id,
                x: d.x,
                y: d.y,
                area_m2,
                residents_night,
                jobs_day,
                activity_mix_residential: d.residential,
                activity_mix_office: d.office,
                activity_mix_retail: d.retail,
                activity_mix_recreation: d.recreation,
                activity_mix_industrial: d.industrial,
                activity_mix_education: d.education,
                activity_mix_health: d.health,
                centrality_score: d.centrality,
                data_quality_score: quality,
                country_iso2: Some(iso.clone()),
            });
        }
    }

    if demand_cells.is_empty() {
        return Err(format!("generated empty demand for country {iso}"));
    }
    Ok((zones, demand_cells))
}

#[allow(dead_code)]
pub(crate) fn ensure_country_demand_coverage(
    app: &AppHandle,
    manifest: &ProjectManifest,
    scenario: &mut Scenario,
) -> bool {
    let mut unlocked = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2)
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            unlocked.insert(code);
        }
    }

    let mut changed = false;
    for iso in unlocked {
        let same_country_cells = scenario
            .world
            .demand_cells
            .iter()
            .filter(|c| {
                c.country_iso2
                    .as_deref()
                    .map(|v| v.eq_ignore_ascii_case(&iso))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        let has_countrywide_ids = same_country_cells
            .iter()
            .any(|c| c.cell_id.starts_with(&format!("dc:{iso}:")));
        let all_bootstrap_style = !same_country_cells.is_empty()
            && same_country_cells
                .iter()
                .all(|c| c.cell_id.starts_with("dc:") || c.cell_id.starts_with("df:"));
        let needs_generation =
            same_country_cells.is_empty() || (all_bootstrap_style && !has_countrywide_ids);
        if !needs_generation {
            continue;
        }

        let start_for_country = manifest
            .start_location
            .as_ref()
            .filter(|s| s.country_iso2.eq_ignore_ascii_case(&iso));
        let generated = match synthesize_country_demand(app, &iso, start_for_country) {
            Ok(v) => v,
            Err(_) => continue,
        };

        scenario.world.demand_cells.retain(|c| {
            let same_country = c
                .country_iso2
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case(&iso))
                .unwrap_or(false);
            !(same_country && (c.cell_id.starts_with("dc:") || c.cell_id.starts_with("df:")))
        });
        scenario.world.zones.retain(|z| {
            let same_country = z
                .country_iso2
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case(&iso))
                .unwrap_or(false);
            !(same_country && (z.id.starts_with("z:dc:") || z.id.starts_with("z:df:")))
        });
        scenario.world.zones.extend(generated.0);
        scenario.world.demand_cells.extend(generated.1);
        changed = true;
    }
    changed
}

#[allow(dead_code)]
pub(crate) fn has_significant_variation(values: &[f64]) -> bool {
    if values.len() < 6 {
        return false;
    }
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        return false;
    }
    (max_v - min_v) > (max_v.abs().max(1.0) * 0.08)
}

#[allow(dead_code)]
pub(crate) fn demand_variation_is_healthy(scenario: &Scenario) -> bool {
    if !scenario.world.demand_cells.is_empty() {
        let residents = scenario
            .world
            .demand_cells
            .iter()
            .map(|c| c.residents_night)
            .collect::<Vec<_>>();
        let jobs = scenario
            .world
            .demand_cells
            .iter()
            .map(|c| c.jobs_day)
            .collect::<Vec<_>>();
        return has_significant_variation(&residents) && has_significant_variation(&jobs);
    }
    let residents = scenario
        .world
        .zones
        .iter()
        .map(|z| z.population)
        .collect::<Vec<_>>();
    let jobs = scenario
        .world
        .zones
        .iter()
        .map(|z| z.jobs)
        .collect::<Vec<_>>();
    has_significant_variation(&residents) && has_significant_variation(&jobs)
}

pub(crate) fn default_scenario_template(
    name: &str,
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> Scenario {
    let (zones, demand_cells) =
        synthesize_city_demand(center_lon, center_lat, city_population, country_iso2);
    let country = country_iso2
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2);
    Scenario {
        meta: Meta {
            name: name.to_string(),
            seed: 42,
            time_period_hours: 1.0,
            crs: Crs::Epsg3857,
        },
        params: default_params(),
        world: World {
            zones,
            stops: vec![],
            links: vec![],
            services: vec![],
            transfers: vec![],
            transfer_rules: None,
            demand_cells,
            demand_meta: Some(DemandMeta {
                surface_version: "legacy-bootstrap".to_string(),
                loaded_countries: country.clone().into_iter().collect(),
                source: "legacy_synthetic".to_string(),
            }),
        },
    }
}

pub(crate) fn default_template_doc(project_name: &str) -> ScenarioDocument {
    ScenarioDocument::new_current(default_scenario_template(
        project_name,
        -1.5491,
        53.8008,
        None,
        None,
    ))
}

pub(crate) fn default_template_doc_at_location(
    project_name: &str,
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> ScenarioDocument {
    ScenarioDocument::new_current(default_scenario_template(
        project_name,
        center_lon,
        center_lat,
        city_population,
        country_iso2,
    ))
}

pub(crate) fn ensure_game_bootstrap_network(
    _scenario: &mut Scenario,
    _center_lon: f64,
    _center_lat: f64,
    _city_population: Option<u64>,
    _country_iso2: Option<&str>,
) -> bool {
    false
}

pub(crate) fn rehydrate_game_state_scenario(
    game_state: &mut interlinked_engine::platform::GameState,
    scenario: &Scenario,
) {
    let previous_tick_s = game_state.tick_s;
    let previous_state = game_state.sim_state.clone();
    let previous_run_cfg = game_state.run_cfg.clone();
    let valid_keys = scenario
        .world
        .services
        .iter()
        .flat_map(|service| {
            service
                .stop_sequence
                .iter()
                .cloned()
                .map(move |stop_id| (service.id.clone(), stop_id))
        })
        .collect::<HashSet<_>>();
    let doc = ScenarioDocument::new_current(scenario.clone());
    let mut next = SimulationService::init_game_state(&doc);
    next.tick_s = previous_tick_s;
    next.sim_state.t_s = previous_state.t_s;
    next.run_cfg = previous_run_cfg;
    next.run_cfg.deterministic_seed = Some(scenario.meta.seed);
    for (key, value) in previous_state.queue {
        if valid_keys.contains(&key) {
            next.sim_state.queue.insert(key, value);
        }
    }
    for (key, value) in previous_state.time_to_next_departure_s {
        if valid_keys.contains(&key) {
            next.sim_state.time_to_next_departure_s.insert(key, value);
        }
    }
    *game_state = next;
}

pub(crate) fn clock_minute_of_day(clock: &SimulationClock) -> u32 {
    let base_minute = clock
        .sim_datetime_utc
        .split('T')
        .nth(1)
        .and_then(|tail| tail.split('Z').next())
        .and_then(|time_part| {
            let mut parts = time_part.split(':');
            let hour = parts.next()?.parse::<u32>().ok()?;
            let minute = parts.next()?.parse::<u32>().ok()?;
            Some((hour % 24) * 60 + (minute % 60))
        })
        .unwrap_or(8 * 60);
    let delta_minutes = (clock.tick_seconds / 60.0).floor() as i64;
    ((base_minute as i64 + delta_minutes).rem_euclid(1440)) as u32
}

pub(crate) fn run_ephemeral_inspection_output(
    scenario: &Scenario,
    apply_game_runtime_overrides: bool,
) -> Result<SimulationOutput, String> {
    let doc = ScenarioDocument::new_current(scenario.clone());
    let mut clone = SimulationService::init_game_state(&doc);
    clone.run_cfg.lightweight_outputs = false;
    let mut materialized = clone.store.scenario().clone();
    if apply_game_runtime_overrides {
        strip_auto_reverse_runtime_artifacts(&mut materialized);
        apply_game_runtime_demand_tuning(&mut materialized.params);
        synthesize_auto_reverse_runtime_services(&mut materialized);
    }
    materialize_line_operations_for_minute(&mut materialized, &economy_config(), 8 * 60);
    clone.store = ScenarioStore::new(materialized);
    let _ = SimulationService::step_game(
        &mut clone,
        300.0,
        interlinked_engine::platform::GameStepRequest {
            recompute_quick_kpis: true,
            edits: Vec::new(),
            force_strategic_refresh: true,
        },
    )?;
    clone
        .last_output
        .ok_or_else(|| "inspection analysis did not produce simulation output".to_string())
}

pub(crate) fn inspection_output_for_project(
    state: &tauri::State<AppState>,
    project_path: &str,
    scenario: &Scenario,
) -> Result<SimulationOutput, String> {
    let apply_game_runtime_overrides = read_manifest(Path::new(project_path))
        .map(|manifest| manifest.session_kind == SessionKind::Game)
        .unwrap_or(false);
    if project_is_current(state, project_path)? {
        let guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_ref() {
            if let Some(output) = game_state.last_output.clone() {
                if !output.meta.results_version.ends_with("-lite") {
                    return Ok(output);
                }
            }
            let mut clone = game_state.clone();
            clone.run_cfg.lightweight_outputs = false;
            drop(guard);
            let mut materialized = clone.store.scenario().clone();
            if apply_game_runtime_overrides {
                strip_auto_reverse_runtime_artifacts(&mut materialized);
                apply_game_runtime_demand_tuning(&mut materialized.params);
                synthesize_auto_reverse_runtime_services(&mut materialized);
            }
            materialize_line_operations_for_minute(&mut materialized, &economy_config(), 8 * 60);
            clone.store = ScenarioStore::new(materialized);
            let _ = SimulationService::step_game(
                &mut clone,
                300.0,
                interlinked_engine::platform::GameStepRequest {
                    recompute_quick_kpis: true,
                    edits: Vec::new(),
                    force_strategic_refresh: true,
                },
            )?;
            return clone.last_output.ok_or_else(|| {
                "inspection analysis did not produce simulation output".to_string()
            });
        }
    }
    run_ephemeral_inspection_output(scenario, apply_game_runtime_overrides)
}
