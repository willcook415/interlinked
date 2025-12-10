using System;
using Interlinked.Core;        // so we can see TimeService
using TMPro;
using UnityEngine;
using UnityEngine.UI;

namespace Interlinked.UI
{
    /// <summary>
    /// Simple controller for the top-left time UI.
    /// Shows current game time and lets the player change time scale.
    /// </summary>
    public class TimeUIController : MonoBehaviour
    {
        [Header("UI References")]
        [SerializeField] private TMP_Text timeLabel;
        [SerializeField] private Button pauseButton;
        [SerializeField] private Button speed1xButton;
        [SerializeField] private Button speed2xButton;
        [SerializeField] private Button speed4xButton;
        [SerializeField] private Button speed8xButton;
        [SerializeField] private Button speed16xButton;

        [Header("Time Display Format")]
        [SerializeField] private string timeFormat = "yyyy-MM-dd HH:mm:ss";

        [Header("Speed Button Colors")]
        [SerializeField] private Color speedNormalColor = new Color32(224, 229, 235, 255);
        [SerializeField] private Color speedActiveColor = new Color32(153, 171, 188, 255);

        private float _currentSpeed = 1f; // tracks which speed is active

        private void Awake()
        {
            // Basic safety check
            if (timeLabel == null)
                Debug.LogWarning("[TimeUIController] Time label is not assigned.");

            // Hook up button events
            if (pauseButton != null) pauseButton.onClick.AddListener(OnPauseClicked);
            if (speed1xButton != null) speed1xButton.onClick.AddListener(() => OnSpeedClicked(1f));
            if (speed2xButton != null) speed2xButton.onClick.AddListener(() => OnSpeedClicked(2f));
            if (speed4xButton != null) speed4xButton.onClick.AddListener(() => OnSpeedClicked(4f));
            if (speed8xButton != null) speed8xButton.onClick.AddListener(() => OnSpeedClicked(8f));
            if (speed16xButton != null) speed16xButton.onClick.AddListener(() => OnSpeedClicked(16f));

            _currentSpeed = 1f;
            SetSpeedButtonColors();
        }

        private void OnDestroy()
        {
            // Clean up listeners when this object is destroyed
            if (pauseButton != null) pauseButton.onClick.RemoveListener(OnPauseClicked);
            if (speed1xButton != null) speed1xButton.onClick.RemoveAllListeners();
            if (speed2xButton != null) speed2xButton.onClick.RemoveAllListeners();
            if (speed4xButton != null) speed4xButton.onClick.RemoveAllListeners();
            if (speed8xButton != null) speed8xButton.onClick.RemoveAllListeners();
            if (speed16xButton != null) speed16xButton.onClick.RemoveAllListeners();
        }

        private void Update()
        {
            // Update the label every frame (cheap, it's just one string)
            if (TimeService.Instance == null || timeLabel == null)
                return;

            timeLabel.text = TimeService.Instance.GetTimestampString(timeFormat);
        }

        private void OnPauseClicked()
        {
            if (TimeService.Instance == null)
                return;

            TimeService.Instance.TogglePause();

            // If we just paused, set current speed to 0 (no button active)
            if (TimeService.Instance.IsPaused)
            {
                _currentSpeed = 0f;
            }
            else
            {
                // If we unpaused from 0, default back to 1x
                if (_currentSpeed <= 0f)
                    _currentSpeed = 1f;
            }

            SetSpeedButtonColors();
        }


        private void OnSpeedClicked(float newScale)
        {
            if (TimeService.Instance == null)
                return;

            TimeService.Instance.SetTimeScale(newScale);
            _currentSpeed = newScale;
            SetSpeedButtonColors();
        }


        private void SetSpeedButtonColors()
        {
            SetButtonColor(speed1xButton, _currentSpeed == 1f);
            SetButtonColor(speed2xButton, _currentSpeed == 2f);
            SetButtonColor(speed4xButton, _currentSpeed == 4f);
            SetButtonColor(speed8xButton, _currentSpeed == 8f);
            SetButtonColor(speed16xButton, _currentSpeed == 16f);
        }

        private void SetButtonColor(Button button, bool active)
        {
            if (button == null) return;

            var colors = button.colors;
            colors.normalColor = active ? speedActiveColor : speedNormalColor;
            button.colors = colors;
        }

    }
}
