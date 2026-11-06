using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Sonda de la UI dinámica (Canvas v2) sobre el texto de puntaje
    /// de UiDynTest: en Start cambia el texto, el tamaño y el color
    /// por las clases nuevas (Text) y mueve el elemento con
    /// RectTransform, anotando los códigos del puente.
    /// </summary>
    public class UiDynTextProbe : GlassBehaviour
    {
        public override void Start()
        {
            int code = RuntimeBridge.UiSetText(EntityId, "UI DINAMICA 42");
            var go = GameObject.GetOrCreate(EntityId, "Score HUD Text");
            var text = go.GetComponent<Text>() ?? go.AddComponent<Text>();
            text.text = "UI DINAMICA 42";
            text.fontSize = 36f;
            text.color = new Color(1f, 0.9f, 0.2f, 1f);
            var rect = go.GetComponent<RectTransform>() ?? go.AddComponent<RectTransform>();
            rect.anchoredPosition = new Vector2(150f, 70f);
            Debug.Log($"[UiDynTextProbe] entidad {EntityId}: UiSetText -> {code}; texto/tamaño/color/posición aplicados.");
        }
    }

    /// <summary>
    /// Sonda de la UI dinámica sobre el icono de UiDynTest: tiñe la
    /// imagen de verde, la mueve y la encoge con Image y
    /// RectTransform, anotando los códigos del puente.
    /// </summary>
    public class UiDynImageProbe : GlassBehaviour
    {
        public override void Start()
        {
            int code = RuntimeBridge.UiSetImageColor(EntityId, 0.25f, 1f, 0.25f, 1f);
            var go = GameObject.GetOrCreate(EntityId, "Icono Prueba");
            var img = go.GetComponent<Image>() ?? go.AddComponent<Image>();
            img.color = new Color(0.25f, 1f, 0.25f, 1f);
            var rect = go.GetComponent<RectTransform>() ?? go.AddComponent<RectTransform>();
            rect.anchoredPosition = new Vector2(-170f, -60f);
            rect.sizeDelta = new Vector2(150f, 150f);
            Debug.Log($"[UiDynImageProbe] entidad {EntityId}: UiSetImageColor -> {code}; tinte/posición/tamaño aplicados.");
        }
    }
}
