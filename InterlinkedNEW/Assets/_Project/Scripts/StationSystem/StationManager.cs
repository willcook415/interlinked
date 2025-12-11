using System.Collections.Generic;
using UnityEngine;
using Interlinked.Map;
using System;


namespace Interlinked.StationSystem
{
    /// <summary>
    /// Singleton-style registry for all stations in the world.
    /// Responsible for:
    /// - Creating stations with unique IDs
    /// - Tracking them by ID
    /// - Handling selection state
    /// </summary>
    public class StationManager : MonoBehaviour
    {
        public static StationManager Instance { get; private set; }

        [Header("Prefabs")]
        [SerializeField] private Station stationPrefab;

        private readonly Dictionary<int, Station> _stationsById = new Dictionary<int, Station>();
        private int _nextStationId = 1;

        private Station _selectedStation;

        public IReadOnlyDictionary<int, Station> Stations => _stationsById;

        // ADD THIS:
        public bool ClickStartedOnStation { get; private set; }

        public event Action<Station> OnSelectedStationChanged;

        public Station SelectedStation => _selectedStation;

        private void Awake()
        {
            if (Instance != null && Instance != this)
            {
                Destroy(gameObject);
                return;
            }

            Instance = this;
        }

        /// <summary>
        /// Create a station at the given world position.
        /// </summary>
        public Station CreateStation(Vector2 worldPosition, StationType type = StationType.BusStop)
        {
            if (stationPrefab == null)
            {
                Debug.LogError("[StationManager] Station prefab is not assigned!");
                return null;
            }

            int id = _nextStationId++;
            string name = $"Station {id}";

            Vector3 spawnPosition = new Vector3(worldPosition.x, worldPosition.y, 0f);

            Station stationInstance = Instantiate(stationPrefab, spawnPosition, Quaternion.identity);
            stationInstance.Init(id, name, type);

            _stationsById.Add(id, stationInstance);

            Debug.Log($"[StationManager] Created {name} at {worldPosition}");

            return stationInstance;
        }

        public Station GetStationById(int id)
        {
            _stationsById.TryGetValue(id, out var station);
            return station;
        }

        /// <summary>
        /// Called by Station.OnMouseDown when a station is clicked.
        /// Handles selection highlighting.
        /// </summary>
        public void HandleStationClicked(Station station)
        {
            if (station == null)
                return;

            // Mark that this click began on a station
            ClickStartedOnStation = true;

            if (_selectedStation == station)
            {
                // Toggle off if clicking the already-selected station
                SetSelectedStation(null);
            }
            else
            {
                SetSelectedStation(station);
            }
        }



        private void SetSelectedStation(Station station)
        {
            if (_selectedStation != null)
                _selectedStation.SetSelected(false);

            _selectedStation = station;

            if (_selectedStation != null)
            {
                _selectedStation.SetSelected(true);

                // Center camera on this station
                var mainCam = Camera.main;
                if (mainCam != null)
                {
                    var camController = mainCam.GetComponent<CameraController2D>();
                    if (camController != null)
                    {
                        camController.FocusOn(_selectedStation.transform.position, true);
                    }
                }
            }

            // Notify any listeners (e.g. StationInfoPanel)
            OnSelectedStationChanged?.Invoke(_selectedStation);
        }

        public void DeleteStation(Station station)
        {
            if (station == null)
                return;

            // If it's selected, clear selection (this also notifies the UI)
            if (_selectedStation == station)
            {
                SetSelectedStation(null);
            }

            // Remove from registry
            if (_stationsById.ContainsKey(station.StationId))
            {
                _stationsById.Remove(station.StationId);
            }

            // Destroy the GameObject
            Destroy(station.gameObject);
        }

        public void DeleteStationById(int id)
        {
            if (_stationsById.TryGetValue(id, out var station))
            {
                DeleteStation(station);
            }
        }

        public void ClearSelection()
        {
            SetSelectedStation(null);
        }



        public void ClearClickStartedFlag()
        {
            ClickStartedOnStation = false;
        }


    }
}
