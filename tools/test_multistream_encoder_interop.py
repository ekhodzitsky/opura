"""Fail-closed synthetic interoperability workload metadata; no native library."""
from collections import Counter
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import multistream_encoder_interop as interop


class MultistreamExportIntegrity(unittest.TestCase):
    def test_predeclared_matrix_keeps_rates_apps_and_real_loss_steps(self):
        protocols=interop.case_protocols()
        self.assertEqual(len(protocols),229)
        self.assertEqual(sum(len(frames) for _,frames in protocols),7405)
        self.assertEqual(Counter(g[0] for g,_ in protocols),{8000:45,12000:45,16000:45,24000:45,48000:49})
        self.assertEqual(Counter(g[3] for g,_ in protocols),{0:45,1:49,2:45,3:45,4:45})
        self.assertEqual(Counter(row[0] for _,frames in protocols for row in frames),{'normal':7309,'plc':48,'fec':48})
        self.assertEqual(protocols[0][0],(8000,1,0,0,[0]))
        self.assertEqual(protocols[-1][0],(48000,5,3,1,[0,6,1,2,3,4,5,7]))
        for geometry,frames in protocols:
            if geometry[3]==3:self.assertTrue(all(row[1]>=480 for row in frames))

    def test_surround_protocol_keeps_canonical_layouts_and_distinct_header(self):
        protocols=interop.case_protocols(True)
        self.assertEqual(len(protocols),204)
        self.assertEqual(sum(len(frames) for _,frames in protocols),6760)
        self.assertTrue(all(len(g[4])<=8 and len(g[4])==g[1]+g[2] for g,_ in protocols))
        self.assertEqual(Counter(row[0] for _,frames in protocols for row in frames),{'normal':6664,'plc':48,'fec':48})
        library=SimpleNamespace(opus_multistream_decoder_destroy=lambda _:None)
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'vectors'
            path.write_text('# OPURA-MULTISTREAM-ENCODE01 test\n')
            with self.assertRaisesRegex(ValueError,'Wrong export'):interop.verify(library,path,'test',True)
            path.write_text('# OPURA-SURROUND-ENCODE01 test\ncase 8000 2 1 0 0,2,1\n')
            with patch.object(interop,'create') as create:
                with self.assertRaisesRegex(ValueError,'geometry/order'):interop.verify(library,path,'test',True)
                create.assert_not_called()

    def reject(self,text,message):
        library=SimpleNamespace(opus_multistream_decoder_destroy=lambda _:None)
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'vectors';path.write_text(text)
            with patch.object(interop,'create',return_value=object()),patch.object(interop,'decode') as decode:
                with self.assertRaisesRegex(ValueError,message):interop.verify(library,path,'test')
                decode.assert_not_called()

    def test_wrong_case_geometry_and_duplicate_empty_cases_cannot_count(self):
        header='# OPURA-MULTISTREAM-ENCODE01 test\n'
        for case in ['case 12000 1 0 0 0\n','case 8000 1 0 1 0\n','case 8000 1 1 0 0,1\n']:
            with self.subTest(case=case):self.reject(header+case,'geometry/order differs')
        self.reject(header+'case 8000 1 0 0 0\ncase 8000 1 0 0 0\n','Empty/incomplete')
        self.reject(header+'case 8000 1 0 0 0\n','Empty/incomplete')

    def test_changed_controls_or_skipped_frame_schedule_fail_before_decode(self):
        header='# OPURA-MULTISTREAM-ENCODE01 test\ncase 8000 1 0 0 0\n'
        # Expected first row: normal/2.5ms,7664byte cap,32000bps,CBR,noFEC/noDTX.
        for fields in [
            'normal 120 7664 64000 0 0 0 0',
            'normal 240 7664 32000 0 0 0 0',
            'normal 120 120 32000 1 0 0 0',
            'normal 120 7664 32000 0 0 1 0',
            'normal 120 7664 32000 0 0 0 1',
            'plc 120 7664 32000 0 0 0 0',
        ]:
            with self.subTest(fields=fields):
                self.reject(header+f'frame {fields} 0 20 0 80\n','schedule/controls differ')
        self.reject(header+'frame normal 120 0 32000 0 0 0 0 0 20 0 80\n','Invalid frame controls')
        self.reject(header+'frame normal 120 7664 32000 2 0 0 0 0 20 0 80\n','Invalid frame controls')


if __name__=='__main__':unittest.main()
