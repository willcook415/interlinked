using System;
using System.Collections.Generic;
using TMPro;
using UnityEngine;

namespace Interlinked.StationSystem
{
    public enum StationType
    {
        BusStop,
        TramStop,
        MetroStation,
        RailStation,
        Interchange
    }

    [Serializable]
    public class PassengerData
    {
        [Tooltip("Estimated daily boardings at this station.")]
        public int dailyBoardingsEstimate;

        [Tooltip("Estimated daily alightings at this station.")]
        public int dailyAlightingsEstimate;

        [Tooltip("Average passenger wait time in minutes (for analytics).")]
        public float averageWaitTimeMinutes;
    }

    /// <summary>
    /// Represents a single station node in the network.
    /// Holds identity, type, passenger stats and line memberships.
    /// Handles its own visual selection state + label text.
    /// </summary>
    [DisallowMultipleComponent]
    public class Station : MonoBehaviour
    {
        [Header("Identity")]
        [SerializeField] private int stationId;
        [SerializeField] private string stationName;
        [SerializeField] private StationType stationType = StationType.BusStop;

        [Header("Line Memberships")]
        [Tooltip("IDs of lines that serve this station (filled in later).")]
        [SerializeField] private List<int> lineIds = new List<int>();

        [Header("Passenger Data (placeholder for future systems)")]
        [SerializeField] private PassengerData passengerData = new PassengerData();

        [Header("Visuals")]
        [SerializeField] private SpriteRenderer iconRenderer;
        [SerializeField] private TMP_Text label;
        [SerializeField] private Color normalColor = new Color32(224, 229, 235, 255);
        [SerializeField] private Color selectedColor = new Color32(153, 171, 188, 255);

        private bool _isSelected;

        #region Public API

        public int StationId => stationId;
        public string StationName => stationName;
        public StationType Type => stationType;
        public IReadOnlyList<int> LineIds => lineIds;
        public PassengerData PassengerStats => passengerData;
        public bool IsSelected => _isSelected;

        public void Init(int id, string name, StationType type)
        {
            stationId = id;
            stationName = name;
            stationType = type;
            UpdateLabel();
        }

        public void Rename(string newName)
        {
            stationName = newName;
            UpdateLabel();
        }

        public void AddLineMembership(int lineId)
        {
            if (!lineIds.Contains(lineId))
                lineIds.Add(lineId);
        }

        public void RemoveLineMembership(int lineId)
        {
            if (lineIds.Contains(lineId))
                lineIds.Remove(lineId);
        }

        public void SetSelected(bool selected)
        {
            _isSelected = selected;
            ApplyColor();
        }

        #endregion

        private void Awake()
        {
            if (iconRenderer == null)
                iconRenderer = GetComponent<SpriteRenderer>();

            ApplyColor();
            UpdateLabel();
        }

        private void OnValidate()
        {
            // Keep visuals & label in sync in editor
            ApplyColor();
            UpdateLabel();
        }

        private void ApplyColor()
        {
            if (iconRenderer != null)
                iconRenderer.color = _isSelected ? selectedColor : normalColor;
        }

        private void UpdateLabel()
        {
            if (label != null)
                label.text = stationName;
        }

        // Handles click/select behaviour (requires Collider/Collider2D)
        private void OnMouseDown()
        {
            if (!enabled) return;
            StationManager.Instance?.HandleStationClicked(this);
        }
    }
}
