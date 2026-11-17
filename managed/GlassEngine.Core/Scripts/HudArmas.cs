using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>HUD de la demo FPS: arma actual y munición en el Text autorado de la entidad 230.</summary>
    public class HudArmas : GlassBehaviour
    {
        private const ulong IdTextoHud = 230;

        private Text _texto;
        private string _ultimoTexto = null;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(IdTextoHud, "HudArma");
            _texto = go.GetComponent<Text>() ?? go.AddComponent<Text>();
        }

        public override void Update(float dt)
        {
            if (_texto == null) return;

            var jugador = JugadorPersonaje.Instancia;
            string s = "";
            if (jugador != null && !string.IsNullOrEmpty(jugador.ArmaActualNombre))
                s = jugador.ArmaActualNombre + "   " + jugador.MunicionEnCargador + " / " + jugador.CargadorActual;

            if (s == _ultimoTexto) return;
            _ultimoTexto = s;
            _texto.text = s;
        }
    }
}
