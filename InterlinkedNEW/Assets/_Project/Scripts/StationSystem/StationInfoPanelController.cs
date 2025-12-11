using Interlinked.Map;
using TMPro;
using UnityEngine;
using UnityEngine.UI;

namespace Interlinked.StationSystem
{
    /// <summary>
    /// Controls the Station Info UI panel.
    /// Listens to StationManager for selection changes and updates fields.
    /// Supports inline name editing with an Edit button.
    /// </summary>
    public class StationInfoPanelController : MonoBehaviour
    {
        [Header("Panel Root")]
        [SerializeField] private GameObject panelRoot;

        [Header("Header")]
        [SerializeField] private TMP_Text nameText;
        [SerializeField] private TMP_Text subtitleText;
        [SerializeField] private Button centerButton;
        [SerializeField] private Button editNameButton;
        [SerializeField] private TMP_InputField nameInput;
        [SerializeField] private TMP_Text editNameButtonLabel;

        [Header("Basic Info")]
        [SerializeField] private TMP_Text positionText;
        [SerializeField] private TMP_Text typeText;

        [Header("Passenger Stats")]
        [SerializeField] private TMP_Text dailyBoardingsText;
        [SerializeField] private TMP_Text dailyAlightingsText;
        [SerializeField] private TMP_Text avgWaitText;

        [Header("Lines")]
        [SerializeField] private TMP_Text linesText;

        [Header("Actions")]
        [SerializeField] private Button deleteButton;


        private Station _currentStation;
        private bool _isSubscribed;
        private bool _isEditingName;

        private void Awake()
        {
            if (centerButton != null)
                centerButton.onClick.AddListener(OnCenterClicked);

            if (editNameButton != null)
                editNameButton.onClick.AddListener(OnEditNameClicked);

            if (deleteButton != null)
                deleteButton.onClick.AddListener(OnDeleteClicked);


            // Start hidden until a station is selected
            if (panelRoot != null)
                panelRoot.SetActive(false);

            // Ensure we start in "view" mode, not editing
            SetEditingMode(false);
        }

        private void OnEnable()
        {
            TrySubscribe();
        }

        private void Start()
        {
            // In case OnEnable ran before StationManager.Awake()
            TrySubscribe();
        }

        private void OnDisable()
        {
            if (_isSubscribed && StationManager.Instance != null)
            {
                StationManager.Instance.OnSelectedStationChanged -= HandleSelectedStationChanged;
                _isSubscribed = false;
            }

            if (centerButton != null)
                centerButton.onClick.RemoveListener(OnCenterClicked);

            if (editNameButton != null)
                editNameButton.onClick.RemoveListener(OnEditNameClicked);

            if (deleteButton != null)
                deleteButton.onClick.AddListener(OnDeleteClicked);

        }

        private void TrySubscribe()
        {
            if (_isSubscribed)
                return;

            if (StationManager.Instance == null)
                return;

            StationManager.Instance.OnSelectedStationChanged += HandleSelectedStationChanged;
            _isSubscribed = true;

            // Sync with current selection if any
            HandleSelectedStationChanged(StationManager.Instance.SelectedStation);
        }

        private void HandleSelectedStationChanged(Station station)
        {
            _currentStation = station;

            if (panelRoot == null)
                return;

            if (_currentStation == null)
            {
                panelRoot.SetActive(false);
                SetEditingMode(false);
                return;
            }

            panelRoot.SetActive(true);
            SetEditingMode(false); // always start in view mode for a new selection
            UpdatePanelContents();
        }

        private void UpdatePanelContents()
        {
            if (_currentStation == null)
                return;

            // Header
            string displayName = string.IsNullOrEmpty(_currentStation.StationName)
                ? $"Station {_currentStation.StationId}"
                : _currentStation.StationName;

            if (nameText != null)
                nameText.text = displayName;

            if (nameInput != null)
            {
                // Keep input in sync without firing OnEndEdit
                nameInput.SetTextWithoutNotify(displayName);
            }

            if (subtitleText != null)
            {
                subtitleText.text =
                    $"ID: {_currentStation.StationId} \u2022 Type: {_currentStation.Type}";
            }

            // Basic info
            if (positionText != null)
            {
                Vector3 pos = _currentStation.transform.position;
                positionText.text = $"Position: ({pos.x:0.0}, {pos.y:0.0})";
            }

            if (typeText != null)
                typeText.text = $"Type: {_currentStation.Type}";

            // Passenger stats (placeholder values from PassengerData)
            var stats = _currentStation.PassengerStats;
            if (dailyBoardingsText != null)
                dailyBoardingsText.text =
                    $"Estimated Daily Boardings: {stats.dailyBoardingsEstimate}";

            if (dailyAlightingsText != null)
                dailyAlightingsText.text =
                    $"Estimated Daily Alightings: {stats.dailyAlightingsEstimate}";

            if (avgWaitText != null)
                avgWaitText.text =
                    $"Avg Wait Time: {stats.averageWaitTimeMinutes:0.0} min";

            // Lines section
            if (linesText != null)
            {
                var lineIds = _currentStation.LineIds;
                if (lineIds == null || lineIds.Count == 0)
                    linesText.text = "Lines: (none)";
                else
                    linesText.text = $"Lines: {string.Join(", ", lineIds)}";
            }
        }

        private void SetEditingMode(bool editing)
        {
            _isEditingName = editing;

            if (nameText != null)
                nameText.gameObject.SetActive(!editing);

            if (nameInput != null)
                nameInput.gameObject.SetActive(editing);

            if (editNameButtonLabel != null)
                editNameButtonLabel.text = editing ? "Save" : "Edit";
        }


        private void OnEditNameClicked()
        {
            if (_currentStation == null || nameInput == null)
                return;

            if (!_isEditingName)
            {
                // Enter edit mode
                SetEditingMode(true);

                string displayName = string.IsNullOrEmpty(_currentStation.StationName)
                    ? $"Station {_currentStation.StationId}"
                    : _currentStation.StationName;

                nameInput.SetTextWithoutNotify(displayName);
                nameInput.Select();
                nameInput.ActivateInputField();
            }
            else
            {
                // Currently editing -> treat button as "Save"
                OnNameEdited(nameInput.text);
            }
        }


        private void OnNameEdited(string newName)
        {
            if (_currentStation == null)
                return;

            string trimmed = newName?.Trim();

            if (string.IsNullOrEmpty(trimmed))
            {
                // If cleared, fall back to default
                trimmed = $"Station {_currentStation.StationId}";
            }

            _currentStation.Rename(trimmed);
            UpdatePanelContents();

            // Return to view mode
            SetEditingMode(false);
        }

        private void OnCenterClicked()
        {
            if (_currentStation == null)
                return;

            var mainCam = Camera.main;
            if (mainCam == null) return;

            var camController = mainCam.GetComponent<CameraController2D>();
            if (camController == null) return;

            camController.FocusOn(_currentStation.transform.position, true);
        }

        private void OnDeleteClicked()
        {
            if (_currentStation == null)
                return;

            if (StationManager.Instance == null)
                return;

            StationManager.Instance.DeleteStation(_currentStation);
            // After this:
            // - Station is destroyed
            // - Selection is cleared
            // - Panel will hide via HandleSelectedStationChanged(null)
        }

    }
}
