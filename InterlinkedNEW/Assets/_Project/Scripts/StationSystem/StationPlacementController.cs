using Interlinked.StationSystem;
using UnityEngine;
using UnityEngine.EventSystems;

namespace Interlinked.StationSystem
{
    /// <summary>
    /// Handles player input for placing stations on the map.
    /// - Left-click on empty map to place a station
    /// - Prevents placement when clicking over UI
    /// </summary>
    public class StationPlacementController : MonoBehaviour
    {
        [Header("General")]
        [SerializeField] private Camera targetCamera;
        [SerializeField] private bool placementEnabled = true;
        [SerializeField] private StationType defaultStationType = StationType.BusStop;
        private bool _clickStartedOverUI;

        [Header("Placement Rules")]
        [SerializeField] private float minStationSpacing = 1.0f; // world units

        private bool _hadSelectionOnMouseDown;


        private void Awake()
        {
            if (targetCamera == null)
                targetCamera = Camera.main;
        }


        private void Update()
        {
            if (!placementEnabled)
                return;

            // --- Mouse Down: record if we started over UI ---
            if (Input.GetMouseButtonDown(0))
            {
                _clickStartedOverUI = false;

                // Remember if we *had* a selection when this click started
                _hadSelectionOnMouseDown = StationManager.Instance != null &&
                                           StationManager.Instance.SelectedStation != null;

                // If we clicked UI, don't do anything else
                if (EventSystem.current != null &&
                    EventSystem.current.IsPointerOverGameObject())
                {
                    _clickStartedOverUI = true;
                    return;
                }

                if (targetCamera != null && StationManager.Instance != null)
                {
                    Vector3 mouseScreenPosDown = Input.mousePosition;
                    Vector3 worldPos3Down = targetCamera.ScreenToWorldPoint(mouseScreenPosDown);
                    Vector2 worldPosDown = new Vector2(worldPos3Down.x, worldPos3Down.y);

                    Collider2D hitDown = Physics2D.OverlapPoint(worldPosDown);

                    // If we didn't hit a station, clear selection
                    if (hitDown == null || hitDown.GetComponent<Station>() == null)
                    {
                        StationManager.Instance.ClearSelection();
                    }
                    // If we DID hit a station, its OnMouseDown will handle selection.
                }
            }



            // --- Mouse Up: decide whether to place a station ---
            if (Input.GetMouseButtonUp(0))
            {
                bool startedOnStation = StationManager.Instance != null &&
                                        StationManager.Instance.ClickStartedOnStation;

                // If the click started on UI or a station, never place
                if (_clickStartedOverUI || startedOnStation)
                {
                    _clickStartedOverUI = false;

                    if (StationManager.Instance != null)
                        StationManager.Instance.ClearClickStartedFlag();

                    return;
                }

                if (_hadSelectionOnMouseDown)
                {
                    _clickStartedOverUI = false;
                    _hadSelectionOnMouseDown = false;
                    StationManager.Instance.ClearClickStartedFlag();
                    return;
                }

                _clickStartedOverUI = false;
                _hadSelectionOnMouseDown = false;

                if (EventSystem.current != null &&
                    EventSystem.current.IsPointerOverGameObject())
                {
                    if (StationManager.Instance != null)
                        StationManager.Instance.ClearClickStartedFlag();
                    return;
                }

                if (targetCamera == null || StationManager.Instance == null)
                    return;

                Vector3 mouseScreenPos = Input.mousePosition;
                Vector3 worldPos3 = targetCamera.ScreenToWorldPoint(mouseScreenPos);
                Vector2 worldPos = new Vector2(worldPos3.x, worldPos3.y);

                // Extra safety: don't place if we're over a station at release either
                Collider2D hit = Physics2D.OverlapPoint(worldPos);
                if (hit != null && hit.GetComponent<Station>() != null)
                {
                    StationManager.Instance.ClearClickStartedFlag();
                    return;
                }

                // NEW: don't place if too close to any existing station
                if (IsTooCloseToExistingStation(worldPos))
                {
                    StationManager.Instance.ClearClickStartedFlag();
                    return;
                }

                StationManager.Instance.CreateStation(worldPos, defaultStationType);
                StationManager.Instance.ClearClickStartedFlag();
            }
        }

        private bool IsTooCloseToExistingStation(Vector2 worldPos)
        {
            if (StationManager.Instance == null)
                return false;

            // Use the manager's registry
            foreach (var kvp in StationManager.Instance.Stations)
            {
                var st = kvp.Value;
                if (st == null) continue;

                Vector2 existingPos = st.transform.position;
                float dist = Vector2.Distance(worldPos, existingPos);

                if (dist < minStationSpacing)
                {
                    return true;
                }
            }

            return false;
        }


    }
}
