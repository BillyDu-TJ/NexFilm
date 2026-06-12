import os
import json
import numpy as np

input_dir = r"E:\Code\NegativeConverter\reference_divere\config\curves"
output_dir = r"E:\Code\NegativeConverter\assets\luts"

if not os.path.exists(output_dir):
    os.makedirs(output_dir)

# Clear existing json files in output_dir
for filename in os.listdir(output_dir):
    if filename.endswith(".json"):
        os.remove(os.path.join(output_dir, filename))

for filename in os.listdir(input_dir):
    if filename.endswith(".json"):
        filepath = os.path.join(input_dir, filename)
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                data = json.load(f)
            
            curves = data.get('curves', {})
            formatted_curves = {}
            if 'R' in curves: formatted_curves['R'] = curves['R']
            if 'G' in curves: formatted_curves['G'] = curves['G']
            if 'B' in curves: formatted_curves['B'] = curves['B']
            
            if not formatted_curves and 'RGB' in curves:
                formatted_curves['R'] = curves['RGB']
                formatted_curves['G'] = curves['RGB']
                formatted_curves['B'] = curves['RGB']
                
            out_filename = filename.replace('.json', '.cube')
            out_filepath = os.path.join(output_dir, out_filename)
            
            # Generate 1D LUT points
            size = 1024
            lut_data = np.zeros((size, 3), dtype=np.float32)
            x_vals = np.linspace(0, 1, size)
            
            for channel_idx, channel_name in enumerate(['R', 'G', 'B']):
                if channel_name in formatted_curves:
                    pts = np.array(formatted_curves[channel_name])
                    lut_data[:, channel_idx] = np.interp(x_vals, pts[:, 0], pts[:, 1])
                else:
                    lut_data[:, channel_idx] = x_vals
                    
            # Save to cube
            with open(out_filepath, 'w') as f:
                f.write(f"TITLE \"DiVERE {filename.replace('.json', '')}\"\n")
                f.write(f"LUT_1D_SIZE {size}\n")
                f.write("DOMAIN_MIN 0.0 0.0 0.0\n")
                f.write("DOMAIN_MAX 1.0 1.0 1.0\n\n")
                for i in range(size):
                    r, g, b = lut_data[i]
                    f.write(f"{r:.6f} {g:.6f} {b:.6f}\n")
                    
            print(f"Translated {filename} to {out_filename}")
        except Exception as e:
            print(f"Error on {filename}: {e}")
