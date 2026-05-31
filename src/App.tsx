import { HashRouter as Router, Routes, Route } from "react-router-dom";
import Settings from "./Settings";
import FloatingIndicator from "./FloatingIndicator";
import ScreenshotOverlay from "./ScreenshotOverlay";
import "./App.css";

function App() {
  return (
    <Router>
      <Routes>
        <Route path="/" element={<Settings />} />
        <Route path="/indicator" element={<FloatingIndicator />} />
        <Route path="/screenshot-overlay" element={<ScreenshotOverlay initialMode="screenshot" />} />
        <Route path="/ocr-overlay" element={<ScreenshotOverlay initialMode="ocr" />} />
      </Routes>
    </Router>
  );
}

export default App;
