using UnityEngine;

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

        private Camera _camera;
        private Vector3 _lastMousePosition;

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
        }

        private void Update()
        {
            HandlePan();
            HandleZoom();

            if (limitBounds)
            {
                ClampToBounds();
            }
        }

        #region Pan Logic

        private void HandlePan()
        {
            // Keyboard pan (WASD / arrow keys)
            if (enableKeyboardPan)
            {
                float h = Input.GetAxisRaw("Horizontal"); // A/D or Left/Right
                float v = Input.GetAxisRaw("Vertical");   // W/S or Up/Down

                Vector3 direction = new Vector3(h, v, 0f).normalized;

                if (direction.sqrMagnitude > 0.001f)
                {
                    // Scale pan speed slightly with zoom so it feels consistent
                    float zoomScale = _camera.orthographicSize * zoomPanFactor;
                    float speed = basePanSpeed * (1f + zoomScale);

                    transform.position += direction * speed * Time.unscaledDeltaTime;
                }
            }

            // Mouse drag pan
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

                    // Convert screen delta to world delta
                    // Screen height vs orthographic size gives us world units per pixel.
                    float worldUnitsPerPixel = (_camera.orthographicSize * 2f) / Screen.height;
                    Vector3 worldDelta = -mouseDelta * worldUnitsPerPixel;

                    transform.position += new Vector3(worldDelta.x, worldDelta.y, 0f);
                }
            }
        }

        #endregion

        #region Zoom Logic

        private void HandleZoom()
        {
            float scroll = Input.GetAxis("Mouse ScrollWheel");
            if (Mathf.Abs(scroll) < 0.0001f)
                return;

            float currentSize = _camera.orthographicSize;

            // Scroll > 0 means zoom in; < 0 means zoom out
            float zoomChange = -scroll * zoomSpeed * (currentSize * 0.2f);
            float newSize = Mathf.Clamp(currentSize + zoomChange, minZoom, maxZoom);

            _camera.orthographicSize = newSize;
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
        /// Instantly move camera to focus on a world position (keeps current Z).
        /// </summary>
        public void FocusOn(Vector3 worldPosition)
        {
            transform.position = new Vector3(worldPosition.x, worldPosition.y, transform.position.z);
        }

        #endregion
    }
}
