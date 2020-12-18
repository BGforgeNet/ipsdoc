#!/usr/bin/env python3

import argparse
import wave
import struct
import platform
import shutil
import subprocess
import sys, os
import ipsdoc_bin
import base64


parser = argparse.ArgumentParser(description='Convert sound file to ACM', formatter_class=argparse.ArgumentDefaultsHelpFormatter)
parser.add_argument('ifile', help='input WAV, ACM or 8/16 bit raw PCM file')
parser.add_argument('ofile', help='output ACM file', nargs="?")
args = parser.parse_args()

ifile = args.ifile
ofile = args.ofile
if ofile is None:
  ofile = ifile.lower().rstrip("wav") + "acm"
  print("output file not specified, defaulting to {}".format(ofile))

acm_channels_off = 8
acm_channels_len = 2
acm_bitrate_off = 10
acm_bitrate_len = 2

snd2acm_name = "snd2acm.exe"

def check_reqs():
  system = platform.system()
  if system != 'Windows':
    wine = shutil.which("wine")
    if wine is None:
      print("wine not present in PATH, aborting")
      sys.exit(1)

def get_exe_string(src_path, dst_path):
  cur_dir = os.path.dirname(sys.argv[0])
  system = platform.system()
  wine = False
  exe = False
  if dst_path.lower().endswith(".acm"):
    exe = os.path.join(cur_dir, snd2acm_name)
    exe = exe + " " + src_path + " " + dst_path + " -q0"
  if not exe:
    print("Unknown output file format, aborting")
    sys.exit(1)
  if system != 'Windows':
    wine = shutil.which("wine")
    exe = wine + " " + exe
  return exe

def get_wav_params(fname):
  with wave.open(fname, mode=None) as wav:
    wav_rate = wav.getframerate()
    wav_channels = wav.getnchannels()
  print("source bitrate: {}, channels: {}".format(wav_rate, wav_channels))
  return {"rate": wav_rate, "channels": wav_channels}

def get_acm_params(fname):
  with open(fname, 'rb') as ofh:
    odata = ofh.read()
  bitrate = odata[acm_bitrate_off:acm_bitrate_off+acm_bitrate_len]
  acm_rate = struct.unpack('<H', bitrate)[0]
  channels = odata[acm_channels_off:acm_channels_off+acm_channels_len]
  acm_channels = struct.unpack('<H', channels)[0]
  print("output bitrate: {}, channels: {}".format(acm_rate, acm_channels))
  return {"rate": acm_rate, "channels": acm_channels}

def fix_acm_params(src_params, fname):
  dst_params = get_acm_params(fname)
  if src_params["rate"] != dst_params["rate"]:
    bitrate = struct.pack('<H', src_params["rate"])
    with open(fname, 'r+b') as fh:
      fh.seek(acm_bitrate_off)
      fh.write(bitrate)
    print("rate mismatch corrected")
  if src_params["channels"] != dst_params["channels"]:
    channels = struct.pack('<H', dst_params["channels"])
    with open(fname, 'r+b') as fh:
      fh.seek(acm_channels_off)
      fh.write(channels)
    print("channels mismatch corrected")

def convert(src_path, dst_path):
  exe = get_exe_string(src_path, dst_path)
  subprocess.run(exe, shell=True)

def create_bin():
  cur_dir = os.path.dirname(sys.argv[0])
  snd2acm_path = os.path.join(cur_dir, snd2acm_name)
  if not os.path.isfile(snd2acm_path):
    with open(snd2acm_path, "wb") as fh:
      fh.write(base64.b64decode(ipsdoc_bin.snd2acm_base64))

check_reqs()
create_bin()
convert(ifile, ofile)
wav_params = get_wav_params(ifile)
fix_acm_params(wav_params, ofile)
