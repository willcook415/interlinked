using UnityEngine;
using Interlinked.StationSystem;
using TMPro;
using UnityEngine.EventSystems;
using UnityEngine.UI;


namespace Interlinked.Map
{
    /// <summary>
    /// 2D camera controller for Interlinked.
    /// - Orthographic camera
    /// - Pan with WASD / arrow keys
    /// - Pan with mouse drag
    /// - Zoom with scroll wheel (clamped)
    /// - Helper to focus on a world position
    /// </summary>
    [RequireComponent(typeof(Camera))]
    public class CameraController2D : MonoBehaviour
    {
        [Header("Pan Settings")]
        [SerializeField] private bool enableKeyboardPan = true;
        [SerializeField] private bool enableMouseDragPan = true;
        [SerializeField] private float basePanSpeed = 10f;
        [Tooltip("Multiplier applied to pan speed based on zoom level so movement feels consistent.")]
        [SerializeField] private float zoomPanFactor = 0.1f;
        [Tooltip("Mouse button index for drag (1 = right, 2 = middle).")]
        [SerializeField] private int dragMouseButton = 2;

        [Header("Zoom Settings")]
        [SerializeField] private float zoomSpeed = 5f;
        [SerializeField] private float minZoom = 3f;
        [SerializeField] private float maxZoom = 30f;

        [Header("Optional World Bounds")]
        [SerializeField] private bool limitBounds = false;
        [Tooltip("Minimum X/Y position the camera can move to (world units).")]
        [SerializeField] private Vector2 minPosition = new Vector2(-100f, -100f);
        [Tooltip("Maximum X/Y position the camera can move to (world units).")]
        [SerializeField] private Vector2 maxPosition = new Vector2(100f, 100f);

        [Header("Momentum Settings")]
        [SerializeField] private bool enablePanMomentum = true;
        [SerializeField] private float momentumDamping = 3f;          // how quickly velocity decays
        [SerializeField] private float momentumAcceleration = 10f;    // how quickly we accelerate towards input
        [SerializeField] private float maxPanSpeed = 100f;

        [Header("Edge Scrolling")]
        [SerializeField] private bool enableEdgeScroll = true;
        [SerializeField] private float edgeSizePixels = 10f;          // distance from screen edge to start scrolling
        [SerializeField] private float edgePanSpeedMultiplier = 1f;   // multiplier for edge speed vs keyboard

        [Header("Smooth Zoom")]
        [SerializeField] private float zoomSmoothSpeed = 10f;

        [Header("Focus / Centering")]
        [SerializeField] private float focusSmoothSpeed = 8f;


        private Camera _camera;
        private Vector3 _lastMousePosition;

        private float _targetZoom;
        private Vector3 _velocity;

        private bool _hasFocusTarget;
        private Vector3 _focusTarget;

        private bool IsTextInputFocused()
        {
            if (EventSystem.current == null)
                return false;

            var go = EventSystem.current.currentSelectedGameObject;
            if (go == null) return false;

            // If a TMP_InputField or standard InputField is selected, we consider it text editing
            return go.GetComponent<TMP_InputField>() != null ||
                   go.GetComponent<InputField>() != null;
        }


        private void Awake()
        {
            _camera = GetComponent<Camera>();


            if (_camera == null)
            {
                Debug.LogError("[CameraController2D] No Camera component found!");
                enabled = false;
                return;
            }

            // Ensure camera is orthographic for 2D view
            _camera.orthographic = true;

            _targetZoom = _camera.orthographicSize;
        }

        private void Update()
        {
            HandlePan();
            HandleZoom();
            ApplyMomentum();
            ApplyFocus();

            if (limitBounds)
            {
                ClampToBounds();
            }
        }

        #region Pan Logic

        private void HandlePan()
        {
            Vector3 moveInputDir = Vector3.zero;

            bool blockKeyboardPan = IsTextInputFocused();

            // Keyboard pan (WASD / arrow keys)
            if (enableKeyboardPan && !blockKeyboardPan)
            {
                float h = Input.GetAxisRaw("Horizontal"); // A/D or Left/Right
                float v = Input.GetAxisRaw("Vertical");   // W/S or Up/Down

                Vector3 keyboardDir = new Vector3(h, v, 0f);
                moveInputDir += keyboardDir;
            }

            // Edge scrolling (only when not dragging)
            if (enableEdgeScroll && !Input.GetMouseButton(dragMouseButton))
            {
                Vector3 mousePos = Input.mousePosition;
                Vector3 edgeDir = Vector3.zero;

                if (mousePos.x <= edgeSizePixels) edgeDir.x -= 1f;
                else if (mousePos.x >= Screen.width - edgeSizePixels) edgeDir.x += 1f;

                if (mousePos.y <= edgeSizePixels) edgeDir.y -= 1f;
                else if (mousePos.y >= Screen.height - edgeSizePixels) edgeDir.y += 1f;

                moveInputDir += edgeDir * edgePanSpeedMultiplier;
            }

            // Apply keyboard/edge pan as velocity or direct movement
            if (moveInputDir.sqrMagnitude > 0.001f)
            {
                moveInputDir.Normalize();

                // Zoom-dependent pan speed: faster when zoomed out, slower when zoomed in
                float speed = basePanSpeed * (_camera.orthographicSize * zoomPanFactor);

                if (enablePanMomentum)
                {
                    Vector3 targetVelocity = moveInputDir * speed;
                    targetVelocity = Vector3.ClampMagnitude(targetVelocity, maxPanSpeed);

                    _velocity = Vector3.Lerp(_velocity, targetVelocity,
                        Time.unscaledDeltaTime * momentumAcceleration);
                }
                else
                {
                    transform.position += moveInputDir * speed * Time.unscaledDeltaTime;
                }
            }

            // Mouse drag pan (direct movement, also feeds momentum)
            if (enableMouseDragPan)
            {
                if (Input.GetMouseButtonDown(dragMouseButton))
                {
                    _lastMousePosition = Input.mousePosition;
                }

                if (Input.GetMouseButton(dragMouseButton))
                {
                    Vector3 mouseDelta = Input.mousePosition - _lastMousePosition;
                    _lastMousePosition = Input.mousePosition;

                    // Screen height vs orthographic size gives us world units per pixel.
                    float worldUnitsPerPixel = (_camera.orthographicSize * 2f) / Screen.height;
                    Vector3 worldDelta = -mouseDelta * worldUnitsPerPixel;

                    transform.position += new Vector3(worldDelta.x, worldDelta.y, 0f);

                    if (enablePanMomentum)
                    {
                        // Approximate velocity from drag so it can "coast" after release
                        _velocity = worldDelta / Time.unscaledDeltaTime;
                        _velocity = Vector3.ClampMagnitude(_velocity, maxPanSpeed);
                    }
                }
            }
        }

        private void ApplyMomentum()
        {
            if (!enablePanMomentum)
                return;

            if (_velocity.sqrMagnitude < 0.0001f)
            {
                _velocity = Vector3.zero;
                return;
            }

            transform.position += _velocity * Time.unscaledDeltaTime;

            // Dampen velocity over time so it eases to a stop
            _velocity = Vector3.Lerp(_velocity, Vector3.zero,
                Time.unscaledDeltaTime * momentumDamping);
        }

        private void ApplyFocus()
        {
            if (!_hasFocusTarget)
                return;

            // Smoothly move towards the focus target
            transform.position = Vector3.Lerp(
                transform.position,
                _focusTarget,
                Time.unscaledDeltaTime * focusSmoothSpeed);

            // If we're very close, snap to exact and stop focusing
            if (Vector3.SqrMagnitude(transform.position - _focusTarget) < 0.0001f)
            {
                transform.position = _focusTarget;
                _hasFocusTarget = false;
                _velocity = Vector3.zero; // stop any momentum overshoot
            }
        }



        #endregion

        #region Zoom Logic

        private void HandleZoom()
        {
            float scroll = Input.GetAxis("Mouse ScrollWheel");

            if (Mathf.Abs(scroll) > 0.0001f)
            {
                // Use target zoom for consistent scroll behaviour
                float currentTarget = _targetZoom;

                // Scroll > 0 means zoom in; < 0 means zoom out
                float zoomChange = -scroll * zoomSpeed * (currentTarget * 0.2f);

                _targetZoom = Mathf.Clamp(currentTarget + zoomChange, minZoom, maxZoom);
            }

                // Smoothly interpolate current zoom towards target
                 _camera.orthographicSize = Mathf.Lerp(
                _camera.orthographicSize,
                _targetZoom,
                Time.unscaledDeltaTime * zoomSmoothSpeed);
        }


        #endregion

        #region Bounds

        private void ClampToBounds()
        {
            Vector3 pos = transform.position;

            float clampedX = Mathf.Clamp(pos.x, minPosition.x, maxPosition.x);
            float clampedY = Mathf.Clamp(pos.y, minPosition.y, maxPosition.y);

            transform.position = new Vector3(clampedX, clampedY, pos.z);
        }

        #endregion

        #region Public API


        /// <summary>
        /// Move camera to focus on a world position (keeps current Z).
        /// If smooth is true, lerps towards the target.
        /// </summary>
        public void FocusOn(Vector3 worldPosition, bool smooth = true)
        {
            Vector3 targetPos = new Vector3(worldPosition.x, worldPosition.y, transform.position.z);

            if (smooth)
            {
                _focusTarget = targetPos;
                _hasFocusTarget = true;
                _velocity = Vector3.zero; // don't fight momentum
            }
            else
            {
                _hasFocusTarget = false;
                transform.position = targetPos;
                _velocity = Vector3.zero;
            }
        }


        /// <summary>
        /// Convenience overload for focusing on a station.
        /// </summary>
        public void FocusOn(Station station, bool smooth = true)
        {
            if (station == null) return;
            FocusOn(station.transform.position, smooth);
        }


        #endregion
    }
}
