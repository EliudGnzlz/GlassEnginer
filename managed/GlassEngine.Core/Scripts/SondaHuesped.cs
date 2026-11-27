using System;
using UnityEngine;

/// <summary>
/// Sonda del huésped de MonoBehaviour: registra cada mensaje de Unity
/// que recibe, para comprobar el orden del ciclo de vida.
/// </summary>
public class SondaHuesped : MonoBehaviour
{
    private new void Awake() => Debug.Log("[SONDA] " + nameof(Awake));

    private void OnEnable() => Debug.Log("[SONDA] " + nameof(OnEnable));

    private new void Start() => Debug.Log("[SONDA] " + nameof(Start));

    private new void FixedUpdate() => Debug.Log("[SONDA] " + nameof(FixedUpdate));

    private new void Update() => Debug.Log("[SONDA] " + nameof(Update));

    private new void LateUpdate() => Debug.Log("[SONDA] " + nameof(LateUpdate));

    private void OnDisable() => Debug.Log("[SONDA] " + nameof(OnDisable));

    private new void OnDestroy() => Debug.Log("[SONDA] " + nameof(OnDestroy));

    private void OnCollisionEnter(Collision collision) => Debug.Log("[SONDA] " + nameof(OnCollisionEnter));

    private void OnCollisionStay(Collision collision) => Debug.Log("[SONDA] " + nameof(OnCollisionStay));

    private void OnCollisionExit(Collision collision) => Debug.Log("[SONDA] " + nameof(OnCollisionExit));

    private void OnTriggerEnter(Collider other) => Debug.Log("[SONDA] " + nameof(OnTriggerEnter));

    private void OnTriggerStay(Collider other) => Debug.Log("[SONDA] " + nameof(OnTriggerStay));

    private void OnTriggerExit(Collider other) => Debug.Log("[SONDA] " + nameof(OnTriggerExit));
}
