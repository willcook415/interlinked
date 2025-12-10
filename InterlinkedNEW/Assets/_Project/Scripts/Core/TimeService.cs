using System;
using UnityEngine;

namespace Interlinked.Core
{
    /// <summary>
    /// Global simulation time controller for Interlinked.
    /// - Keeps track of in-game DateTime
    /// - Supports time scaling & pausing
    /// - Raises events when minute/hour/day change
    /// - Provides simple snapshot API for save/load
    /// </summary>
    [DisallowMultipleComponent]
    [DefaultExecutionOrder(-1000)] // Initialise before most other scripts
    public class TimeService : MonoBehaviour
    {
        #region Singleton

        public static TimeService Instance { get; private set; }

        private void Awake()
        {
            if (Instance != null && Instance != this)
            {
                Destroy(gameObject);
                return;
            }

            Instance = this;
            DontDestroyOnLoad(gameObject);
        }

        #endregion

        #region Public API

        /// <summary>
        /// Current in-game time.
        /// </summary>
        public DateTime CurrentTime { get; private set; }

        /// <summary>
        /// How fast in-game time runs relative to real time.
        /// 0 = paused, 1 = real-time, 2 = double speed, etc.
        /// </summary>
        public float TimeScale { get; private set; } = 1f;

        /// <summary>
        /// True if the simulation is currently paused.
        /// </summary>
        public bool IsPaused => TimeScale <= 0f;

        /// <summary>
        /// Fired whenever the minute value changes.
        /// </summary>
        public event Action<DateTime> OnMinuteChanged;

        /// <summary>
        /// Fired whenever the hour value changes.
        /// </summary>
        public event Action<DateTime> OnHourChanged;

        /// <summary>
        /// Fired whenever the day value changes.
        /// </summary>
        public event Action<DateTime> OnDayChanged;

        /// <summary>
        /// Set the time scale. Use 0 to pause.
        /// </summary>
        public void SetTimeScale(float newScale)
        {
            TimeScale = Mathf.Max(0f, newScale);
        }

        public void Pause() => SetTimeScale(0f);

        public void Resume()
        {
            if (TimeScale <= 0f)
                SetTimeScale(1f);
        }

        public void TogglePause()
        {
            if (IsPaused) Resume();
            else Pause();
        }

        /// <summary>
        /// Returns a snapshot that can be stored for save/load.
        /// </summary>
        public TimeSnapshot GetSnapshot()
        {
            return new TimeSnapshot
            {
                ticks = CurrentTime.Ticks,
                timeScale = TimeScale
            };
        }

        /// <summary>
        /// Restores time from a previously captured snapshot.
        /// </summary>
        public void RestoreSnapshot(TimeSnapshot snapshot)
        {
            CurrentTime = new DateTime(snapshot.ticks);
            TimeScale = snapshot.timeScale;
            CacheLastTimeParts(); // keep internal state consistent
        }

        /// <summary>
        /// Returns a formatted timestamp string for UI/logs.
        /// Default: "yyyy-MM-dd HH:mm".
        /// </summary>
        public string GetTimestampString(string format = "yyyy-MM-dd HH:mm")
        {
            return CurrentTime.ToString(format);
        }

        #endregion

        #region Configuration

        [Header("Start Time (Game World)")]
        [SerializeField] private int startYear = 2025;
        [SerializeField] private int startMonth = 1;
        [SerializeField] private int startDay = 1;
        [SerializeField] private int startHour = 4;
        [SerializeField] private int startMinute = 0;

        // If you ever want 1 real second = N in-game seconds, change this.
        [Tooltip("Multiplier for how many in-game seconds pass per real second at TimeScale = 1.")]
        [SerializeField] private float secondsPerRealSecondAtScale1 = 1f;

        #endregion

        #region Internal state

        private int _lastMinute;
        private int _lastHour;
        private int _lastDay;

        #endregion

        private void Start()
        {
            // Initialise to 01/01/2025 04:00 (or whatever you set in inspector)
            CurrentTime = new DateTime(startYear, startMonth, startDay, startHour, startMinute, 0);
            CacheLastTimeParts();
        }

        private void Update()
        {
            if (IsPaused) return;

            // Real seconds passed this frame
            float realDelta = Time.unscaledDeltaTime;

            // Convert to in-game seconds, respecting time scale
            double gameSeconds = realDelta * secondsPerRealSecondAtScale1 * TimeScale;

            if (gameSeconds <= 0d) return;

            DateTime previousTime = CurrentTime;
            CurrentTime = CurrentTime.AddSeconds(gameSeconds);

            CheckForTimeChanges(previousTime, CurrentTime);
        }

        private void CheckForTimeChanges(DateTime previous, DateTime current)
        {
            // Minute change
            if (current.Minute != previous.Minute ||
                current.Hour != previous.Hour ||
                current.Day != previous.Day)
            {
                // Fire minute changed at least once if anything larger changed.
                OnMinuteChanged?.Invoke(current);
            }

            // Hour change
            if (current.Hour != previous.Hour || current.Day != previous.Day)
            {
                OnHourChanged?.Invoke(current);
            }

            // Day change
            if (current.Day != previous.Day || current.Month != previous.Month || current.Year != previous.Year)
            {
                OnDayChanged?.Invoke(current);
            }

            CacheLastTimeParts();
        }

        private void CacheLastTimeParts()
        {
            _lastMinute = CurrentTime.Minute;
            _lastHour = CurrentTime.Hour;
            _lastDay = CurrentTime.Day;
        }

        #region Snapshot struct

        [Serializable]
        public struct TimeSnapshot
        {
            public long ticks;
            public float timeScale;
        }

        #endregion
    }
}
