// A three.js preview module, proving the vendored import map works offline.
//
// This is the shape the tool was built for: parameters you cannot judge from
// numbers — light angle, roughness, emissive strength — driven by sliders with
// the result rendering live at 60fps.
import * as THREE from "three";

let renderer;
let scene;
let camera;
let mesh;
let keyLight;
let rimLight;
let frame;

export default {
  setup({ container, values, emit }) {
    const width = container.clientWidth || 480;
    const height = container.clientHeight || 320;

    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    renderer.setSize(width, height, false);
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.height = "100%";
    container.append(renderer.domElement);

    scene = new THREE.Scene();
    camera = new THREE.PerspectiveCamera(42, width / height, 0.1, 100);
    camera.position.set(2.4, 1.5, 3.2);
    camera.lookAt(0, 0, 0);

    mesh = new THREE.Mesh(
      new THREE.TorusKnotGeometry(0.72, 0.24, 180, 32),
      new THREE.MeshStandardMaterial({ color: "#b0367a", roughness: 0.35, metalness: 0.6 }),
    );
    scene.add(mesh);

    keyLight = new THREE.DirectionalLight(0xffffff, 3);
    rimLight = new THREE.DirectionalLight(0x88bbff, 2);
    rimLight.position.set(-3, 1, -2);
    scene.add(keyLight, rimLight, new THREE.AmbientLight(0xffffff, 0.35));

    const tick = () => {
      frame = requestAnimationFrame(tick);
      if (mesh) mesh.rotation.y += 0.004;
      renderer.render(scene, camera);
    };
    tick();

    // Prove the iframe -> agent channel: this lands in the session event log.
    emit?.("renderer_ready", { webgl2: renderer.capabilities.isWebGL2, three: THREE.REVISION });

    this.update(values);
  },

  update(values) {
    if (!mesh) return;
    const material = mesh.material;
    material.color.set(values.base_colour ?? "#b0367a");
    material.roughness = Number(values.roughness ?? 0.35);
    material.metalness = Number(values.metalness ?? 0.6);
    material.emissive.set(values.emissive_colour ?? "#000000");
    material.emissiveIntensity = Number(values.emissive_strength ?? 0);

    const angle = ((Number(values.light_angle ?? 45)) * Math.PI) / 180;
    const distance = 4;
    keyLight.position.set(Math.cos(angle) * distance, Number(values.light_height ?? 2), Math.sin(angle) * distance);
    keyLight.intensity = Number(values.light_strength ?? 3);
  },

  dispose() {
    cancelAnimationFrame(frame);
    mesh?.geometry.dispose();
    mesh?.material.dispose();
    renderer?.dispose();
    renderer = scene = camera = mesh = undefined;
  },
};
